use crate::config::Hooks;
use shell_words::split;
use std::fmt;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// Represents the different phases where hooks can be executed
#[derive(Debug, Clone, Copy)]
pub enum HookPhase {
    BuildBefore,
    BuildAfter,
    RenderInitBefore,
    RenderInitAfter,
    RenderBefore,
    RenderAfter,
}

impl HookPhase {
    /// Returns the hook name for logging purposes
    fn name(self) -> &'static str {
        match self {
            HookPhase::BuildBefore => "build_before",
            HookPhase::BuildAfter => "build_after",
            HookPhase::RenderInitBefore => "render_init_before",
            HookPhase::RenderInitAfter => "render_init_after",
            HookPhase::RenderBefore => "render_before",
            HookPhase::RenderAfter => "render_after",
        }
    }
}

/// Executes hooks at various phases of the build process
pub struct HookExecutor<'a> {
    hooks: Option<&'a Hooks>,
    base_path: &'a Path,
}

impl<'a> HookExecutor<'a> {
    /// Creates a new `HookExecutor`
    ///
    /// Accepts None for hooks - executor will silently skip all hooks
    #[must_use]
    pub fn new(hooks: Option<&'a Hooks>, base_path: &'a Path) -> Self {
        Self { hooks, base_path }
    }

    /// Gets the command string for a given hook phase
    fn get_hook_command(&self, phase: HookPhase) -> Option<&String> {
        let hooks = self.hooks?;
        match phase {
            HookPhase::BuildBefore => hooks.build_before.as_ref(),
            HookPhase::BuildAfter => hooks.build_after.as_ref(),
            HookPhase::RenderInitBefore => hooks.render_init_before.as_ref(),
            HookPhase::RenderInitAfter => hooks.render_init_after.as_ref(),
            HookPhase::RenderBefore => hooks.render_before.as_ref(),
            HookPhase::RenderAfter => hooks.render_after.as_ref(),
        }
    }

    /// Executes a hook at the specified phase, exiting the process on failure
    ///
    /// Used by `balzac build`, where a failing hook aborts the whole build.
    pub fn execute(&self, phase: HookPhase) {
        if let Err(error) = self.run(phase) {
            log::error!("{error}");
            std::process::exit(1);
        }
    }

    /// Runs a hook at the specified phase
    ///
    /// Returns Ok(()) if the hook is not configured or succeeds, and an error
    /// if its command is invalid or empty, fails to start, or exits with a
    /// non-zero status.
    pub fn run(&self, phase: HookPhase) -> Result<(), HookError> {
        let Some(hook_command) = self.get_hook_command(phase) else {
            return Ok(()); // Hook not configured, silent success
        };

        let hook = phase.name();
        log::info!("Found {hook} hook, running");
        let start = Instant::now();

        let parts = split(hook_command).map_err(|error| HookError::InvalidCommand {
            hook,
            command: hook_command.clone(),
            message: error.to_string(),
        })?;
        let Some((program, args)) = parts.split_first() else {
            return Err(HookError::Empty { hook });
        };

        let status = Command::new(program)
            .args(args)
            .current_dir(self.base_path)
            .status()
            .map_err(|source| HookError::Spawn { hook, source })?;
        if !status.success() {
            return Err(HookError::Failed {
                hook,
                code: status.code(),
            });
        }

        log::info!("{} hook completed (took {:?})", hook, start.elapsed());
        Ok(())
    }
}

/// Why a hook could not run successfully
#[derive(Debug)]
pub enum HookError {
    InvalidCommand {
        hook: &'static str,
        command: String,
        message: String,
    },
    Empty {
        hook: &'static str,
    },
    Spawn {
        hook: &'static str,
        source: std::io::Error,
    },
    Failed {
        hook: &'static str,
        code: Option<i32>,
    },
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookError::InvalidCommand {
                hook,
                command,
                message,
            } => write!(f, "Invalid {hook} hook command {command:?}: {message}"),
            HookError::Empty { hook } => write!(f, "{hook} hook is empty"),
            HookError::Spawn { hook, source } => write!(f, "Error running {hook} hook: {source}"),
            HookError::Failed { hook, code } => {
                write!(f, "{hook} hook failed with exit code: {code:?}")
            }
        }
    }
}

impl std::error::Error for HookError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HookError::Spawn { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Hooks;
    use std::path::PathBuf;

    #[test]
    fn test_hook_phase_names() {
        assert_eq!(HookPhase::BuildBefore.name(), "build_before");
        assert_eq!(HookPhase::BuildAfter.name(), "build_after");
        assert_eq!(HookPhase::RenderInitBefore.name(), "render_init_before");
        assert_eq!(HookPhase::RenderInitAfter.name(), "render_init_after");
        assert_eq!(HookPhase::RenderBefore.name(), "render_before");
        assert_eq!(HookPhase::RenderAfter.name(), "render_after");
    }

    #[test]
    fn test_executor_with_no_hooks_config() {
        let base_path = PathBuf::from("/tmp");
        let executor = HookExecutor::new(None, &base_path);

        // Should not panic or exit
        executor.execute(HookPhase::BuildBefore);
        executor.execute(HookPhase::BuildAfter);
        executor.execute(HookPhase::RenderInitBefore);
        executor.execute(HookPhase::RenderInitAfter);
        executor.execute(HookPhase::RenderBefore);
        executor.execute(HookPhase::RenderAfter);
    }

    #[test]
    fn test_get_hook_command_with_partial_hooks() {
        let hooks = Hooks {
            build_before: Some("echo before".to_string()),
            build_after: None,
            render_init_before: None,
            render_init_after: None,
            render_before: None,
            render_after: None,
            ..Hooks::default()
        };
        let base_path = PathBuf::from("/tmp");
        let executor = HookExecutor::new(Some(&hooks), &base_path);

        assert_eq!(
            executor.get_hook_command(HookPhase::BuildBefore),
            Some(&"echo before".to_string())
        );
        assert_eq!(executor.get_hook_command(HookPhase::BuildAfter), None);
    }

    #[test]
    fn test_get_hook_command_with_all_hooks() {
        let hooks = Hooks {
            build_before: Some("npm run prebuild".to_string()),
            build_after: Some("npm run postbuild".to_string()),
            render_init_before: Some("echo render init before".to_string()),
            render_init_after: Some("echo render init after".to_string()),
            render_before: Some("echo render before".to_string()),
            render_after: Some("echo render after".to_string()),
            ..Hooks::default()
        };
        let base_path = PathBuf::from("/tmp");
        let executor = HookExecutor::new(Some(&hooks), &base_path);

        assert_eq!(
            executor.get_hook_command(HookPhase::BuildBefore),
            Some(&"npm run prebuild".to_string())
        );
        assert_eq!(
            executor.get_hook_command(HookPhase::BuildAfter),
            Some(&"npm run postbuild".to_string())
        );
        assert_eq!(
            executor.get_hook_command(HookPhase::RenderInitBefore),
            Some(&"echo render init before".to_string())
        );
        assert_eq!(
            executor.get_hook_command(HookPhase::RenderInitAfter),
            Some(&"echo render init after".to_string())
        );
        assert_eq!(
            executor.get_hook_command(HookPhase::RenderBefore),
            Some(&"echo render before".to_string())
        );
        assert_eq!(
            executor.get_hook_command(HookPhase::RenderAfter),
            Some(&"echo render after".to_string())
        );
    }

    #[test]
    fn test_execute_missing_hook_returns_silently() {
        let hooks = Hooks {
            build_before: None,
            build_after: None,
            render_init_before: None,
            render_init_after: None,
            render_before: None,
            render_after: None,
            ..Hooks::default()
        };
        let base_path = PathBuf::from("/tmp");
        let executor = HookExecutor::new(Some(&hooks), &base_path);

        // Should not panic or exit
        executor.execute(HookPhase::BuildBefore);
        executor.execute(HookPhase::BuildAfter);
        executor.execute(HookPhase::RenderInitBefore);
        executor.execute(HookPhase::RenderInitAfter);
        executor.execute(HookPhase::RenderBefore);
        executor.execute(HookPhase::RenderAfter);
    }

    #[test]
    fn test_run_reports_failing_hook_instead_of_exiting() {
        let hooks = Hooks {
            build_before: Some("false".to_string()),
            ..Hooks::default()
        };
        let base_path = PathBuf::from("/tmp");
        let executor = HookExecutor::new(Some(&hooks), &base_path);

        let error = executor.run(HookPhase::BuildBefore).unwrap_err();
        assert!(matches!(
            error,
            HookError::Failed {
                hook: "build_before",
                code: Some(1)
            }
        ));
    }

    #[test]
    fn test_run_reports_invalid_and_missing_commands() {
        let hooks = Hooks {
            build_before: Some("echo 'unterminated".to_string()),
            build_after: Some(String::new()),
            render_before: Some("balzac-definitely-not-a-command".to_string()),
            ..Hooks::default()
        };
        let base_path = PathBuf::from("/tmp");
        let executor = HookExecutor::new(Some(&hooks), &base_path);

        assert!(matches!(
            executor.run(HookPhase::BuildBefore),
            Err(HookError::InvalidCommand { .. })
        ));
        assert!(matches!(
            executor.run(HookPhase::BuildAfter),
            Err(HookError::Empty { .. })
        ));
        assert!(matches!(
            executor.run(HookPhase::RenderBefore),
            Err(HookError::Spawn { .. })
        ));
    }
}
