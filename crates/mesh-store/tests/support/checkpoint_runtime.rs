#![allow(dead_code)]

use std::convert::Infallible;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mesh_store::{
    Checkpoint, ChunkPromoter, DurableCommit, PrivateSaved, RecordDigest, RecoverySnapshot,
    RecoveryStatePersistence, Sqlite, Store,
};

#[derive(Clone, Debug, Default)]
pub struct MemoryPersistence {
    state: Arc<Mutex<Option<RecoverySnapshot>>>,
}

impl RecoveryStatePersistence for MemoryPersistence {
    type Error = Infallible;

    fn load(&mut self) -> Result<Option<RecoverySnapshot>, Self::Error> {
        Ok(self.state.lock().expect("persistence lock").clone())
    }

    fn persist(&mut self, snapshot: &RecoverySnapshot) -> Result<(), Self::Error> {
        *self.state.lock().expect("persistence lock") = Some(snapshot.clone());
        Ok(())
    }

    fn persist_observation_recovery(
        &mut self,
        snapshot: &RecoverySnapshot,
    ) -> Result<(), Self::Error> {
        self.persist(snapshot)
    }
}

#[derive(Default)]
struct NoChunks;

impl ChunkPromoter for NoChunks {
    type Error = Infallible;

    fn write_temporary(&mut self, chunks: &[Vec<u8>]) -> Result<Vec<RecordDigest>, Self::Error> {
        assert!(chunks.is_empty());
        Ok(Vec::new())
    }
    fn flush_temporary(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn verify_temporary(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn promote(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn discard_temporary(&mut self) -> Result<usize, Self::Error> {
        Ok(0)
    }
    fn is_durable(&self, _digest: &RecordDigest) -> bool {
        false
    }
}

pub fn private_saved() -> PrivateSaved {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path: PathBuf = std::env::temp_dir().join(format!(
        "mesh-checkpoint-runtime-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_file(&path);
    let sqlite = Sqlite::open(&path).expect("open test database");
    let mut store = Store::open(sqlite).expect("open store");
    let mut chunks = NoChunks;
    let saved = DurableCommit::new(&mut store, &mut chunks, Vec::new(), Checkpoint::default())
        .finish()
        .expect("durable empty checkpoint");
    let acknowledgement = *saved.acknowledgement();
    drop(store);
    let _ = fs::remove_file(path.with_extension("sqlite-shm"));
    let _ = fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs::remove_file(path);
    acknowledgement
}
