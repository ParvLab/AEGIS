use crate::types::Revision;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub type WatchSender = std::sync::mpsc::Sender<WatchEvent>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchEventType {
    TupleAdded,
    TupleRemoved,
}

#[derive(Debug, Clone)]
pub struct WatchEvent {
    pub event_type: WatchEventType,
    pub subject: String,
    pub relation: String,
    pub object: String,
    pub revision: Revision,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct WatchFilter {
    pub subjects: Option<Vec<String>>,
    pub relations: Option<Vec<String>>,
    pub objects: Option<Vec<String>>,
    pub event_types: Option<Vec<WatchEventType>>,
}

impl WatchFilter {
    pub fn matches(&self, event: &WatchEvent) -> bool {
        if let Some(subjects) = &self.subjects {
            if !subjects.iter().any(|s| s == &event.subject) {
                return false;
            }
        }
        if let Some(relations) = &self.relations {
            if !relations.iter().any(|r| r == &event.relation) {
                return false;
            }
        }
        if let Some(objects) = &self.objects {
            if !objects.iter().any(|o| o == &event.object) {
                return false;
            }
        }
        if let Some(types) = &self.event_types {
            if !types.contains(&event.event_type) {
                return false;
            }
        }
        true
    }
}

pub type SharedWatchers = Arc<Mutex<HashMap<Uuid, (WatchFilter, WatchSender)>>>;

pub struct WatchSubscription {
    id: Uuid,
    receiver: std::sync::mpsc::Receiver<WatchEvent>,
    _sender: WatchSender,
    filter: WatchFilter,
    watchers: SharedWatchers,
}

impl WatchSubscription {
    pub(crate) fn new(
        id: Uuid,
        filter: WatchFilter,
        receiver: std::sync::mpsc::Receiver<WatchEvent>,
        sender: WatchSender,
        watchers: SharedWatchers,
    ) -> Self {
        Self {
            id,
            receiver,
            _sender: sender,
            filter,
            watchers,
        }
    }

    pub fn filter(&self) -> &WatchFilter {
        &self.filter
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn try_recv(&self) -> Result<WatchEvent, std::sync::mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn recv(&self) -> Result<WatchEvent, std::sync::mpsc::RecvError> {
        self.receiver.recv()
    }

    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<WatchEvent, std::sync::mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }

    pub fn iter(&self) -> std::sync::mpsc::Iter<'_, WatchEvent> {
        self.receiver.iter()
    }

    pub fn try_iter(&self) -> std::sync::mpsc::TryIter<'_, WatchEvent> {
        self.receiver.try_iter()
    }

    pub fn unsubscribe(&self) {
        if let Ok(mut watchers) = self.watchers.lock() {
            watchers.remove(&self.id);
        }
    }
}

impl Drop for WatchSubscription {
    fn drop(&mut self) {
        self.unsubscribe();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::GraphEngine;
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::storage::StorageBackend;
    use crate::types::*;
    use std::sync::mpsc::TryRecvError;

    fn make_engine() -> GraphEngine {
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types: {
                let mut types = std::collections::HashMap::new();
                let mut relations = std::collections::HashMap::new();
                relations.insert(
                    "owner".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                relations.insert(
                    "viewer".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                let mut permissions = std::collections::HashMap::new();
                permissions.insert(
                    "read".to_string(),
                    crate::types::schema::PermissionDef {
                        union_of: vec!["viewer".to_string(), "owner".to_string()],
                        condition: None,
                        description: None,
                    },
                );
                types.insert(
                    "repo".to_string(),
                    crate::types::schema::TypeDef {
                        relations,
                        permissions,
                    },
                );
                types
            },
        };

        let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
        storage.initialize().unwrap();
        GraphEngine::new(Box::new(storage), schema)
    }

    #[test]
    fn test_watch_receives_write_event() {
        let engine = make_engine();
        let sub = engine.watch_all();

        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:fluxbus").unwrap(),
            ))
            .unwrap();

        let event = sub.recv().unwrap();
        assert_eq!(event.event_type, WatchEventType::TupleAdded);
        assert_eq!(event.subject, "user:alice");
        assert_eq!(event.relation, "owner");
        assert_eq!(event.object, "repo:fluxbus");
        assert!(event.revision.as_u64() > 0);
    }

    #[test]
    fn test_watch_receives_delete_event() {
        let engine = make_engine();
        let sub = engine.watch_all();

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
        );
        engine.write(&tuple).unwrap();
        let _add_event = sub.recv().unwrap();

        engine.delete(&tuple.key()).unwrap();
        let del_event = sub.recv().unwrap();
        assert_eq!(del_event.event_type, WatchEventType::TupleRemoved);
        assert_eq!(del_event.subject, "user:alice");
        assert_eq!(del_event.object, "repo:fluxbus");
    }

    #[test]
    fn test_watch_filter_by_subject() {
        let engine = make_engine();
        let filter = WatchFilter {
            subjects: Some(vec!["user:alice".to_string()]),
            ..Default::default()
        };
        let sub = engine.watch(filter);

        // This write should NOT match the filter
        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:bob").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:other").unwrap(),
            ))
            .unwrap();

        match sub.try_recv() {
            Err(TryRecvError::Empty) => {}
            other => panic!("expected empty channel, got {:?}", other),
        }

        // This write SHOULD match
        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("viewer").unwrap(),
                ResourceId::new("repo:other").unwrap(),
            ))
            .unwrap();

        let event = sub.recv().unwrap();
        assert_eq!(event.subject, "user:alice");
    }

    #[test]
    fn test_watch_filter_by_relation() {
        let engine = make_engine();
        let filter = WatchFilter {
            relations: Some(vec!["owner".to_string()]),
            ..Default::default()
        };
        let sub = engine.watch(filter);

        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("viewer").unwrap(),
                ResourceId::new("repo:fluxbus").unwrap(),
            ))
            .unwrap();

        match sub.try_recv() {
            Err(TryRecvError::Empty) => {}
            other => panic!("expected empty channel, got {:?}", other),
        }

        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:fluxbus").unwrap(),
            ))
            .unwrap();

        let event = sub.recv().unwrap();
        assert_eq!(event.relation, "owner");
    }

    #[test]
    fn test_watch_filter_by_object() {
        let engine = make_engine();
        let filter = WatchFilter {
            objects: Some(vec!["repo:target".to_string()]),
            ..Default::default()
        };
        let sub = engine.watch(filter);

        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:other").unwrap(),
            ))
            .unwrap();

        match sub.try_recv() {
            Err(TryRecvError::Empty) => {}
            other => panic!("expected empty channel, got {:?}", other),
        }

        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:target").unwrap(),
            ))
            .unwrap();

        let event = sub.recv().unwrap();
        assert_eq!(event.object, "repo:target");
    }

    #[test]
    fn test_watch_filter_by_event_type() {
        let engine = make_engine();
        let filter = WatchFilter {
            event_types: Some(vec![WatchEventType::TupleRemoved]),
            ..Default::default()
        };
        let sub = engine.watch(filter);

        let tuple = RelationshipTuple::new(
            SubjectId::new("user:alice").unwrap(),
            Relation::new("owner").unwrap(),
            ResourceId::new("repo:fluxbus").unwrap(),
        );
        engine.write(&tuple).unwrap();

        match sub.try_recv() {
            Err(TryRecvError::Empty) => {}
            other => panic!("expected empty channel, got {:?}", other),
        }

        engine.delete(&tuple.key()).unwrap();
        let event = sub.recv().unwrap();
        assert_eq!(event.event_type, WatchEventType::TupleRemoved);
    }

    #[test]
    fn test_watch_multiple_subscribers() {
        let engine = make_engine();
        let sub1 = engine.watch_all();
        let sub2 = engine.watch_all();

        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:fluxbus").unwrap(),
            ))
            .unwrap();

        let event1 = sub1.recv().unwrap();
        let event2 = sub2.recv().unwrap();
        assert_eq!(event1.subject, "user:alice");
        assert_eq!(event2.subject, "user:alice");
    }

    #[test]
    fn test_watch_unsubscribe_stops_events() {
        let engine = make_engine();
        let sub = engine.watch_all();

        sub.unsubscribe();

        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:fluxbus").unwrap(),
            ))
            .unwrap();

        match sub.try_recv() {
            Err(TryRecvError::Empty) => {}
            other => panic!("expected empty channel, got {:?}", other),
        }
    }

    #[test]
    fn test_watch_drop_removes_subscription() {
        let engine = make_engine();
        let mut subs = vec![];
        for _ in 0..3 {
            subs.push(engine.watch_all());
        }

        // Drop the first and third
        subs.remove(0);
        subs.remove(1);

        // Write an event - should only be delivered to the remaining subscriber
        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:fluxbus").unwrap(),
            ))
            .unwrap();

        // The remaining subscriber should get the event
        let event = subs[0].recv().unwrap();
        assert_eq!(event.subject, "user:alice");

        // Verify only one watcher remains in the engine
        assert_eq!(engine.watchers.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_watch_filter_default_matches_all() {
        let engine = make_engine();
        let sub = engine.watch(WatchFilter::default());

        engine
            .write(&RelationshipTuple::new(
                SubjectId::new("user:alice").unwrap(),
                Relation::new("owner").unwrap(),
                ResourceId::new("repo:fluxbus").unwrap(),
            ))
            .unwrap();

        let event = sub.recv().unwrap();
        assert_eq!(event.subject, "user:alice");
    }
}
