//! CRDT (Conflict-free Replicated Data Type) sync layer.
//!
//! Provides delta-based bi-directional synchronization of relationship tuples
//! across multiple nodes using an OR-Set with version vectors.

use crate::error::{AegisError, AegisResult};
use crate::types::{Relation, RelationshipTuple, ResourceId, SubjectId, TupleKey};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

/// A unique node identifier.
pub type NodeId = Uuid;

/// Version vector tracking per-node operation counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionVector {
    entries: HashMap<NodeId, u64>,
}

impl VersionVector {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn increment(&mut self, node_id: NodeId) -> u64 {
        let counter = self.entries.entry(node_id).or_insert(0);
        *counter += 1;
        *counter
    }

    pub fn get(&self, node_id: &NodeId) -> u64 {
        self.entries.get(node_id).copied().unwrap_or(0)
    }

    /// Merge another version vector into this one (take the max for each entry).
    pub fn merge(&mut self, other: &VersionVector) {
        for (&node, &counter) in &other.entries {
            let entry = self.entries.entry(node).or_insert(0);
            *entry = (*entry).max(counter);
        }
    }

    /// Check if this vector is strictly ahead of another (all entries >=, at least one >).
    pub fn dominates(&self, other: &VersionVector) -> bool {
        let mut has_greater = false;
        for (&node, &counter) in &other.entries {
            let self_val = self.get(&node);
            if self_val < counter {
                return false;
            }
            if self_val > counter {
                has_greater = true;
            }
        }
        // Also check if we have entries the other doesn't
        for &node in self.entries.keys() {
            if !other.entries.contains_key(&node) && self.get(&node) > 0 {
                has_greater = true;
            }
        }
        has_greater
    }

    pub fn is_empty(&self) -> bool {
        self.entries.values().all(|&v| v == 0)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for VersionVector {
    fn default() -> Self {
        Self::new()
    }
}

/// A single CRDT operation (add or remove a tuple).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdtOperation {
    pub node_id: NodeId,
    pub counter: u64,
    pub action: CrdtAction,
    pub subject: String,
    pub relation: String,
    pub object: String,
    pub metadata: Option<HashMap<String, String>>,
    /// The sender's full version vector at the time of the operation.
    pub version: VersionVector,
}

/// The type of CRDT action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrdtAction {
    Add,
    Remove,
}

impl CrdtOperation {
    pub fn new_add(
        node_id: NodeId,
        counter: u64,
        tuple: &RelationshipTuple,
        version: VersionVector,
    ) -> Self {
        Self {
            node_id,
            counter,
            action: CrdtAction::Add,
            subject: tuple.subject.to_string(),
            relation: tuple.relation.to_string(),
            object: tuple.object.to_string(),
            metadata: tuple.metadata.clone(),
            version,
        }
    }

    pub fn new_remove(
        node_id: NodeId,
        counter: u64,
        key: &TupleKey,
        version: VersionVector,
    ) -> Self {
        Self {
            node_id,
            counter,
            action: CrdtAction::Remove,
            subject: key.subject.to_string(),
            relation: key.relation.to_string(),
            object: key.object.to_string(),
            metadata: None,
            version,
        }
    }

    pub fn to_tuple_key(&self) -> AegisResult<TupleKey> {
        let subject = SubjectId::new(&self.subject)
            .map_err(|e| AegisError::SchemaValidation(e.to_string()))?;
        let relation = Relation::new(&self.relation)
            .map_err(|e| AegisError::SchemaValidation(e.to_string()))?;
        let object = ResourceId::new(&self.object)
            .map_err(|e| AegisError::SchemaValidation(e.to_string()))?;
        Ok(TupleKey {
            subject,
            relation,
            object,
        })
    }
}

/// Sync state for a single peer.
#[derive(Debug, Clone)]
struct PeerState {
    node_id: NodeId,
    /// Last known version vector from this peer.
    remote_vector: VersionVector,
    address: String,
}

/// Pluggable transport for sending/receiving CRDT operations.
pub trait SyncTransport: Send + Sync {
    /// Send a batch of operations to a peer.
    fn send_operations(
        &self,
        peer_address: &str,
        ops: &[CrdtOperation],
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Request missing operations from a peer since the given version vector.
    fn request_operations(
        &self,
        peer_address: &str,
        known_vector: &VersionVector,
    ) -> Result<Vec<CrdtOperation>, Box<dyn std::error::Error>>;
}

/// In-memory transport for testing (sends via channel).
pub struct InMemoryTransport {
    receivers: Arc<Mutex<HashMap<NodeId, std::sync::mpsc::Sender<Vec<CrdtOperation>>>>>,
}

impl InMemoryTransport {
    pub fn new() -> Self {
        Self {
            receivers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, node_id: NodeId, tx: std::sync::mpsc::Sender<Vec<CrdtOperation>>) {
        self.receivers.lock().unwrap().insert(node_id, tx);
    }
}

impl SyncTransport for InMemoryTransport {
    fn send_operations(
        &self,
        peer_address: &str,
        ops: &[CrdtOperation],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let node_id: NodeId = peer_address.parse()?;
        let receivers = self.receivers.lock().unwrap();
        if let Some(tx) = receivers.get(&node_id) {
            tx.send(ops.to_vec())?;
        }
        Ok(())
    }

    fn request_operations(
        &self,
        _peer_address: &str,
        _known_vector: &VersionVector,
    ) -> Result<Vec<CrdtOperation>, Box<dyn std::error::Error>> {
        // In-memory transport uses push-based delivery, not pull
        Ok(Vec::new())
    }
}

/// HTTP-based transport for CRDT sync (behind `crdt` feature).
#[cfg(feature = "crdt")]
pub struct HttpSyncTransport {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "crdt")]
impl HttpSyncTransport {
    pub fn new() -> AegisResult<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| AegisError::StorageConnection(e.to_string()))?;
        Ok(Self { client })
    }
}

#[cfg(feature = "crdt")]
impl SyncTransport for HttpSyncTransport {
    fn send_operations(
        &self,
        peer_address: &str,
        ops: &[CrdtOperation],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}/aegis/v1/crdt/operations", peer_address.trim_end_matches('/'));
        let body = serde_json::to_string(ops)?;
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .send()?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()).into());
        }
        Ok(())
    }

    fn request_operations(
        &self,
        peer_address: &str,
        known_vector: &VersionVector,
    ) -> Result<Vec<CrdtOperation>, Box<dyn std::error::Error>> {
        let url = format!("{}/aegis/v1/crdt/sync", peer_address.trim_end_matches('/'));
        let body = serde_json::to_string(known_vector)?;
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .send()?;
        let ops: Vec<CrdtOperation> = response.json()?;
        Ok(ops)
    }
}

/// The CRDT replicator manages peer connections and operation propagation.
pub struct CrdtReplicator {
    node_id: NodeId,
    vector: Arc<RwLock<VersionVector>>,
    pending: Arc<Mutex<Vec<CrdtOperation>>>,
    peers: Arc<RwLock<HashMap<NodeId, PeerState>>>,
    transport: Box<dyn SyncTransport>,
    _applied: Arc<Mutex<HashSet<(NodeId, u64)>>>,
}

impl CrdtReplicator {
    pub fn new(node_id: NodeId, transport: Box<dyn SyncTransport>) -> Self {
        Self {
            node_id,
            vector: Arc::new(RwLock::new(VersionVector::new())),
            pending: Arc::new(Mutex::new(Vec::new())),
            peers: Arc::new(RwLock::new(HashMap::new())),
            transport,
            _applied: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn vector(&self) -> VersionVector {
        self.vector.read().unwrap().clone()
    }

    pub fn peer_count(&self) -> usize {
        self.peers.read().unwrap().len()
    }

    /// Register a peer for synchronization.
    pub fn add_peer(&self, node_id: NodeId, address: String) {
        self.peers.write().unwrap().insert(
            node_id,
            PeerState {
                node_id,
                remote_vector: VersionVector::new(),
                address,
            },
        );
    }

    /// Remove a peer.
    pub fn remove_peer(&self, node_id: &NodeId) {
        self.peers.write().unwrap().remove(node_id);
    }

    pub fn peer_addresses(&self) -> Vec<String> {
        self.peers
            .read()
            .unwrap()
            .values()
            .map(|p| p.address.clone())
            .collect()
    }

    /// Record a local operation. Returns the counter assigned to it.
    pub fn record_operation(&self, op: &CrdtOperation) -> u64 {
        let counter = {
            let mut vector = self.vector.write().unwrap();
            vector.increment(self.node_id)
        };
        let mut pending = self.pending.lock().unwrap();
        pending.push(op.clone());
        counter
    }

    /// Flush all pending operations to all known peers.
    pub fn flush(&self) -> AegisResult<usize> {
        let pending: Vec<CrdtOperation> = {
            let mut p = self.pending.lock().unwrap();
            let ops = p.drain(..).collect();
            ops
        };

        if pending.is_empty() {
            return Ok(0);
        }

        let peers = self.peers.read().unwrap();
        let addresses: Vec<String> = peers.values().map(|p| p.address.clone()).collect();
        drop(peers);

        let mut sent = 0;
        for addr in &addresses {
            if self.transport.send_operations(addr, &pending).is_ok() {
                sent += pending.len();
            }
        }

        Ok(sent)
    }

    /// Apply a batch of remote operations to a local storage backend.
    /// Returns the number of operations applied.
    pub fn apply_remote_operations(
        &self,
        ops: &[CrdtOperation],
        storage: &dyn crate::storage::StorageBackend,
    ) -> AegisResult<usize> {
        let mut applied = 0;
        let mut vector = self.vector.write().unwrap();
        let mut applied_set = self._applied.lock().unwrap();

        for op in ops {
            let key = (op.node_id, op.counter);
            if applied_set.contains(&key) {
                continue;
            }

            // Check if this operation is already known via version vector
            if vector.get(&op.node_id) >= op.counter {
                continue;
            }

            match op.action {
                CrdtAction::Add => {
                    let subject = match SubjectId::new(&op.subject) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let relation = match Relation::new(&op.relation) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    let object = match ResourceId::new(&op.object) {
                        Ok(o) => o,
                        Err(_) => continue,
                    };
                    let tuple = if let Some(ref meta) = op.metadata {
                        match RelationshipTuple::with_metadata(subject, relation, object, meta.clone()) {
                            Ok(t) => t,
                            Err(_) => continue,
                        }
                    } else {
                        RelationshipTuple::new(subject, relation, object)
                    };
                    if storage.write_tuple(&tuple).is_ok() {
                        applied += 1;
                    }
                }
                CrdtAction::Remove => {
                    if let Ok(key) = op.to_tuple_key() {
                        if storage.delete_tuple(&key).is_ok() {
                            applied += 1;
                        }
                    }
                }
            }

            applied_set.insert(key);
            vector.merge(&op.version);
        }

        Ok(applied)
    }

    /// Pull operations from all peers that are ahead of our known state.
    pub fn sync_from_peers(
        &self,
        storage: &dyn crate::storage::StorageBackend,
    ) -> AegisResult<usize> {
        let known = self.vector();
        let peers = self.peers.read().unwrap();
        let addresses: Vec<String> = peers.values().map(|p| p.address.clone()).collect();
        drop(peers);

        let mut total = 0;
        for addr in &addresses {
            if let Ok(ops) = self.transport.request_operations(addr, &known) {
                if !ops.is_empty() {
                    total += self.apply_remote_operations(&ops, storage)?;
                }
            }
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::storage::StorageBackend;
    use std::sync::mpsc;

    #[test]
    fn test_version_vector_increment() {
        let node = NodeId::new_v4();
        let mut vv = VersionVector::new();
        assert_eq!(vv.increment(node), 1);
        assert_eq!(vv.increment(node), 2);
        assert_eq!(vv.get(&node), 2);
    }

    #[test]
    fn test_version_vector_merge() {
        let node_a = NodeId::new_v4();
        let node_b = NodeId::new_v4();

        let mut vv1 = VersionVector::new();
        vv1.increment(node_a);
        vv1.increment(node_a);

        let mut vv2 = VersionVector::new();
        vv2.increment(node_b);
        vv2.increment(node_b);
        vv2.increment(node_b);

        vv1.merge(&vv2);
        assert_eq!(vv1.get(&node_a), 2);
        assert_eq!(vv1.get(&node_b), 3);
    }

    #[test]
    fn test_version_vector_dominates() {
        let node = NodeId::new_v4();
        let mut vv1 = VersionVector::new();
        vv1.increment(node);
        vv1.increment(node);

        let mut vv2 = VersionVector::new();
        vv2.increment(node);

        assert!(vv1.dominates(&vv2));
        assert!(!vv2.dominates(&vv1));
    }

    #[test]
    fn test_in_memory_transport() {
        let node_a = NodeId::new_v4();
        let node_b = NodeId::new_v4();

        let (tx, rx) = mpsc::channel();
        let transport = InMemoryTransport::new();
        transport.register(node_b, tx);

        let replicator_a = CrdtReplicator::new(node_a, Box::new(InMemoryTransport::new()));
        replicator_a.add_peer(node_b, node_b.to_string());

        // Create an operation on node B's transport (simulating node A sending to B)
        let op = CrdtOperation {
            node_id: node_a,
            counter: 1,
            action: CrdtAction::Add,
            subject: "user:alice".to_string(),
            relation: "owner".to_string(),
            object: "repo:fluxbus".to_string(),
            metadata: None,
            version: VersionVector::new(),
        };

        transport
            .send_operations(&node_b.to_string(), &[op])
            .unwrap();

        let received = rx.recv().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].subject, "user:alice");
    }

    #[test]
    fn test_apply_remote_operations() {
        let node_a = NodeId::new_v4();
        let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
        storage.initialize().unwrap();

        let transport = InMemoryTransport::new();
        let replicator = CrdtReplicator::new(node_a, Box::new(transport));

        let ops = vec![CrdtOperation {
            node_id: node_a,
            counter: 1,
            action: CrdtAction::Add,
            subject: "user:alice".to_string(),
            relation: "owner".to_string(),
            object: "repo:fluxbus".to_string(),
            metadata: None,
            version: VersionVector::new(),
        }];

        let applied = replicator
            .apply_remote_operations(&ops, &storage)
            .unwrap();
        assert_eq!(applied, 1);

        let key = TupleKey {
            subject: SubjectId::new("user:alice").unwrap(),
            relation: Relation::new("owner").unwrap(),
            object: ResourceId::new("repo:fluxbus").unwrap(),
        };
        assert!(storage.has_tuple(&key).unwrap());
    }

    #[test]
    fn test_dedup_remote_operations() {
        let node_a = NodeId::new_v4();
        let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
        storage.initialize().unwrap();

        let transport = InMemoryTransport::new();
        let replicator = CrdtReplicator::new(node_a, Box::new(transport));

        let op = CrdtOperation {
            node_id: node_a,
            counter: 1,
            action: CrdtAction::Add,
            subject: "user:alice".to_string(),
            relation: "owner".to_string(),
            object: "repo:fluxbus".to_string(),
            metadata: None,
            version: VersionVector::new(),
        };

        let ops = vec![op.clone()];
        let applied1 = replicator.apply_remote_operations(&ops, &storage).unwrap();
        assert_eq!(applied1, 1);

        // Apply the same operation again - should be deduped
        let applied2 = replicator.apply_remote_operations(&ops, &storage).unwrap();
        assert_eq!(applied2, 0);
    }

    #[test]
    fn test_peer_management() {
        let node_a = NodeId::new_v4();
        let node_b = NodeId::new_v4();
        let transport = InMemoryTransport::new();
        let replicator = CrdtReplicator::new(node_a, Box::new(transport));

        assert_eq!(replicator.peer_count(), 0);
        replicator.add_peer(node_b, "http://localhost:8080".to_string());
        assert_eq!(replicator.peer_count(), 1);
        assert_eq!(replicator.peer_addresses().len(), 1);

        replicator.remove_peer(&node_b);
        assert_eq!(replicator.peer_count(), 0);
    }
}
