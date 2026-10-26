//! Parallel render passes. Once pages are discovered and the `collections`
//! context is built, pages are independent: each one reads its template, renders
//! it with the shared (`Sync`) renderer and writes its own output file.
//!
//! Work runs on rayon's global thread pool, one thread per CPU unless
//! `RAYON_NUM_THREADS` says otherwise. Errors are those of the first failing
//! item in input order, so a build fails the same way whichever thread finishes
//! first.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use rayon::prelude::*;

use crate::error::Result;

/// Runs `f` on every item in parallel. Fails with the error of the first
/// failing item in `items` order; items after it may be skipped.
pub fn try_for_each<T: Sync>(items: &[T], f: impl Fn(&T) -> Result<()> + Sync) -> Result<()> {
    match items
        .par_iter()
        .filter_map(|item| f(item).err())
        .find_first(|_| true)
    {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Maps every item in parallel, keeping their order. Fails with the error of
/// the first failing item in `items` order.
pub fn try_map<T: Sync, U: Send>(
    items: &[T],
    f: impl Fn(&T) -> Result<U> + Sync,
) -> Result<Vec<U>> {
    items
        .par_iter()
        .map(&f)
        .collect::<Vec<_>>()
        .into_iter()
        .collect()
}

/// Creates the parent directories of `paths` before a parallel pass, so pages
/// sharing a directory don't race to create it.
pub fn create_parent_dirs<'a>(paths: impl IntoIterator<Item = &'a Path>) -> std::io::Result<()> {
    let directories: BTreeSet<&Path> = paths.into_iter().filter_map(Path::parent).collect();
    for directory in directories {
        fs::create_dir_all(directory)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use std::path::PathBuf;

    fn failing(index: usize) -> Error {
        Error::Frontmatter {
            path: PathBuf::from(format!("content/posts/{index}.md")),
            message: "boom".to_string(),
        }
    }

    fn failing_path(error: &Error) -> &Path {
        match error {
            Error::Frontmatter { path, .. } => path,
            other => panic!("unexpected error {other}"),
        }
    }

    #[test]
    fn test_try_for_each_reports_the_first_failing_item() {
        let items: Vec<usize> = (0..500).collect();
        for _ in 0..20 {
            let error = try_for_each(&items, |&i| {
                if i % 97 == 13 {
                    Err(failing(i))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert_eq!(failing_path(&error), Path::new("content/posts/13.md"));
        }
    }

    #[test]
    fn test_try_for_each_visits_every_item() {
        let items: Vec<usize> = (0..500).collect();
        let visited = std::sync::atomic::AtomicUsize::new(0);
        try_for_each(&items, |_| {
            visited.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })
        .unwrap();
        assert_eq!(visited.into_inner(), items.len());
    }

    #[test]
    fn test_try_map_keeps_order_and_reports_the_first_failing_item() {
        let items: Vec<usize> = (0..500).collect();
        assert_eq!(try_map(&items, |&i| Ok(i * 2)).unwrap()[499], 998);

        let error =
            try_map(&items, |&i| if i >= 250 { Err(failing(i)) } else { Ok(i) }).unwrap_err();
        assert_eq!(failing_path(&error), Path::new("content/posts/250.md"));
    }

    #[test]
    fn test_create_parent_dirs() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root = temp_dir.path();
        let outputs = [
            root.join("posts/a.html"),
            root.join("posts/b.html"),
            root.join("posts/page/2/index.html"),
            root.join("index.html"),
        ];
        create_parent_dirs(outputs.iter().map(PathBuf::as_path)).unwrap();
        assert!(root.join("posts").is_dir());
        assert!(root.join("posts/page/2").is_dir());
    }
}
