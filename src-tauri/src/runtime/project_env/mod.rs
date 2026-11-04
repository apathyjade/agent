// ── Project Environment: runtime requirements for bound projects ──
//
// Project-level layer on top of the runtime engine. It scans project files for
// runtime requirements and persists bindings in the application database. The
// underlying toolchain engine lives in `crate::runtime::toolchain`.

pub mod detector;
pub mod service;

pub use detector::{
    BoundProject, ProjectDetector, ProjectRuntimeRequirement, ProjectScanResult, SyncAction,
    SyncResult,
};
