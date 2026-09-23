use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum SyncStatus {
    InSync,
    LocallyModified,
    UpstreamModified,
    Conflicted,
    Overridden,
}

#[derive(Debug)]
pub struct FileSyncState {
    pub relative_path: PathBuf,
    pub status: SyncStatus,
}

pub struct SyncEngine;

impl SyncEngine {
    /// Detects sync status between a local file and upstream content
    pub fn compare(local_content: Option<&str>, upstream_content: Option<&str>, has_local_override: bool) -> SyncStatus {
        if has_local_override {
            return SyncStatus::Overridden;
        }

        match (local_content, upstream_content) {
            (Some(local), Some(upstream)) => {
                if local == upstream {
                    SyncStatus::InSync
                } else {
                    SyncStatus::LocallyModified
                }
            }
            (Some(_), None) => SyncStatus::LocallyModified,
            (None, Some(_)) => SyncStatus::UpstreamModified,
            (None, None) => SyncStatus::InSync,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_status_in_sync() {
        let content = "same content";
        assert_eq!(
            SyncEngine::compare(Some(content), Some(content), false),
            SyncStatus::InSync
        );
    }

    #[test]
    fn test_sync_status_locally_modified() {
        assert_eq!(
            SyncEngine::compare(Some("local change"), Some("upstream original"), false),
            SyncStatus::LocallyModified
        );
    }

    #[test]
    fn test_sync_status_overridden() {
        assert_eq!(
            SyncEngine::compare(Some("local change"), Some("upstream original"), true),
            SyncStatus::Overridden
        );
    }
}
