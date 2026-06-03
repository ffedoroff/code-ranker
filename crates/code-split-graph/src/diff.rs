//! Snapshot diffing — **not implemented** in this release. The baseline/diff
//! flow was tied to the old model; it will be reintroduced against the generic
//! model later. Callers (`check --baseline`, `report --baseline`) surface the
//! error returned here.

use crate::snapshot::Snapshot;
use anyhow::{Result, bail};

pub fn compare_snapshots(_before: &Snapshot, _after: &Snapshot) -> Result<()> {
    bail!("snapshot diff (--baseline) is not implemented in this release")
}
