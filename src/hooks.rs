use crate::config::Hooks;
use shell_words::split;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// Represents the different phases where hooks can be executed
#[derive(Debug, Clone, Copy)]
pub enum HookPhase {
    BuildBefore,
    BuildAfter,
}

impl HookPhase {
    /// Returns the hook name for logging purposes
    fn name(&self) -> &'static str {
        match self {
            HookPhase::BuildBefore => "build_before",
            HookPhase::BuildAfter => "build_after",
        }
    }
}

/// Executes hooks at various phases of the build process
pub struct HookExecutor<'a> {
    hooks: Option<&'a Hooks>,
    base_path: &'a Path,
}

impl<'a> HookExecutor<'a> {
    /// Creates a new HookExecutor
    ///
    /// Accepts None for hooks - executor will silently skip all hooks
    pub fn new(hooks: Option<&'a Hooks>, base_path: &'a Path) -> Self {
        Self { hooks, base_path }
    }

    /// Gets the command string for a given hook phase
    fn get_hook_command(&self, phase: HookPhase) -> Option<&String> {
        let hooks = self.hooks?;
        match phase {
            HookPhase::BuildBefore => hooks.build_before.as_ref(),
            HookPhase::BuildAfter => hooks.build_after.as_ref(),
        }
    }

    /// Executes a hook at the specified phase
    ///
    /// Returns Ok(()) if:
    /// - The hook is not configured (silent success)
    /// - The hook executes successfully
    ///
    /// Exits the process with code 1 if:
    /// - The hook command is invalid or empty
    /// - The hook process fails to start
    /// - The hook process exits with non-zero status
    pub fn execute(&self, phase: HookPhase) {
        let Some(hook_command) = self.get_hook_command(phase) else {
            return; // Hook not configured, silent success
        };

        let hook_name = phase.name();
        log::info!("Found {} hook, running", hook_name);
        let start = Instant::now();

        // Parse command
        let parts = split(hook_command).expect("Invalid command syntax");
        if parts.is_empty() {
            log::error!("{} hook is empty", hook_name);
            std::process::exit(1);
        }

        // Build command
        let mut cmd = Command::new(&parts[0]);
        for arg in &parts[1..] {
            cmd.arg(arg);
        }
        cmd.current_dir(self.base_path);

        // Execute and check status
        match cmd.status() {
            Ok(status) => {
                if !status.success() {
                    log::error!(
                        "{} hook failed with exit code: {:?}",
                        hook_name,
                        status.code()
                    );
                    std::process::exit(1);
                }
            }
            Err(e) => {
                log::error!("Error running {} hook: {}", hook_name, e);
                std::process::exit(1);
            }
        }

        log::info!("{} hook completed (took {:?})", hook_name, start.elapsed());
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
    }

    #[test]
    fn test_executor_with_no_hooks_config() {
        let base_path = PathBuf::from("/tmp");
        let executor = HookExecutor::new(None, &base_path);

        // Should not panic or exit
        executor.execute(HookPhase::BuildBefore);
        executor.execute(HookPhase::BuildAfter);
    }

    #[test]
    fn test_get_hook_command_with_partial_hooks() {
        let hooks = Hooks {
            build_before: Some("echo before".to_string()),
            build_after: None,
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
    }

    #[test]
    fn test_execute_missing_hook_returns_silently() {
        let hooks = Hooks {
            build_before: None,
            build_after: None,
        };
        let base_path = PathBuf::from("/tmp");
        let executor = HookExecutor::new(Some(&hooks), &base_path);

        // Should not panic or exit
        executor.execute(HookPhase::BuildBefore);
        executor.execute(HookPhase::BuildAfter);
    }
}
