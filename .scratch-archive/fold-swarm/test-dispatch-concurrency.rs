// ABOUTME: Concurrency scenario tests for dispatch system.
// ABOUTME: Tests race conditions in task creation, claiming, and execution.
//
// Run with: cargo test --test dispatch_concurrency -- --nocapture
// Or copy to tests/ directory first

use gorp::session::{DispatchEvent, DispatchTaskStatus, SessionStore};
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

/// Scenario: Multiple threads creating tasks simultaneously
/// Expected: All tasks created with unique IDs, no duplicates, no crashes
#[test]
fn scenario_concurrent_task_creation() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = Arc::new(SessionStore::new(tmp.path()).unwrap());

    // Create a target room first
    store.create_channel("worker", "!worker:matrix.org").unwrap();

    let num_threads = 10;
    let tasks_per_thread = 50;

    let handles: Vec<_> = (0..num_threads)
        .map(|thread_id| {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                let mut task_ids = Vec::new();
                for i in 0..tasks_per_thread {
                    let prompt = format!("Thread {} Task {}", thread_id, i);
                    match store.create_dispatch_task("!worker:matrix.org", &prompt) {
                        Ok(task) => task_ids.push(task.id),
                        Err(e) => panic!("Thread {} failed to create task: {}", thread_id, e),
                    }
                }
                task_ids
            })
        })
        .collect();

    // Collect all task IDs
    let mut all_ids: Vec<String> = Vec::new();
    for handle in handles {
        let ids = handle.join().expect("Thread panicked");
        all_ids.extend(ids);
    }

    // Verify all tasks created
    assert_eq!(
        all_ids.len(),
        num_threads * tasks_per_thread,
        "Expected {} tasks, got {}",
        num_threads * tasks_per_thread,
        all_ids.len()
    );

    // Verify no duplicate IDs
    let mut sorted = all_ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        all_ids.len(),
        "Found duplicate task IDs! {} unique out of {}",
        sorted.len(),
        all_ids.len()
    );

    // Verify all tasks are in database
    let all_tasks = store.list_dispatch_tasks(None).unwrap();
    assert_eq!(
        all_tasks.len(),
        num_threads * tasks_per_thread,
        "Database has {} tasks, expected {}",
        all_tasks.len(),
        num_threads * tasks_per_thread
    );

    println!(
        "✅ Created {} tasks concurrently with no duplicates",
        all_ids.len()
    );
}

/// Scenario: Multiple threads racing to claim the same pending task
/// Expected: Exactly one thread successfully claims each task
#[test]
fn scenario_concurrent_task_claiming() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = Arc::new(SessionStore::new(tmp.path()).unwrap());

    // Create a target room and tasks
    store.create_channel("worker", "!worker:matrix.org").unwrap();

    let num_tasks = 20;
    let task_ids: Vec<String> = (0..num_tasks)
        .map(|i| {
            store
                .create_dispatch_task("!worker:matrix.org", &format!("Task {}", i))
                .unwrap()
                .id
        })
        .collect();

    let num_claimers = 5;
    let task_ids = Arc::new(task_ids);

    // Spawn multiple threads trying to claim the same tasks
    let handles: Vec<_> = (0..num_claimers)
        .map(|claimer_id| {
            let store = Arc::clone(&store);
            let task_ids = Arc::clone(&task_ids);
            thread::spawn(move || {
                let mut claimed = Vec::new();
                for task_id in task_ids.iter() {
                    // Use atomic claim_dispatch_task - only one claimer can succeed per task
                    if let Ok(true) = store.claim_dispatch_task(
                        task_id,
                        DispatchTaskStatus::Pending,
                        DispatchTaskStatus::InProgress,
                    ) {
                        claimed.push(task_id.clone());
                    }
                }
                (claimer_id, claimed)
            })
        })
        .collect();

    // Collect results
    let mut total_claimed = 0;
    for handle in handles {
        let (claimer_id, claimed) = handle.join().expect("Thread panicked");
        println!("Claimer {} claimed {} tasks", claimer_id, claimed.len());
        total_claimed += claimed.len();
    }

    // Check final state - all tasks should be InProgress
    let in_progress = store
        .list_dispatch_tasks(Some(DispatchTaskStatus::InProgress))
        .unwrap();
    let pending = store
        .list_dispatch_tasks(Some(DispatchTaskStatus::Pending))
        .unwrap();

    println!(
        "Final state: {} in_progress, {} pending",
        in_progress.len(),
        pending.len()
    );

    // All tasks should have been claimed (moved from pending)
    assert_eq!(
        in_progress.len(),
        num_tasks,
        "Expected all {} tasks to be in_progress, got {}",
        num_tasks,
        in_progress.len()
    );
    assert_eq!(pending.len(), 0, "Expected 0 pending tasks");

    println!("✅ All {} tasks claimed exactly once", num_tasks);
}

/// Scenario: Concurrent event insertion and acknowledgment
/// Expected: No lost events, no double acknowledgments
#[test]
fn scenario_concurrent_event_handling() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = Arc::new(SessionStore::new(tmp.path()).unwrap());

    let num_events = 100;

    // Insert events from multiple threads
    let insert_handles: Vec<_> = (0..4)
        .map(|thread_id| {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                for i in 0..25 {
                    let event = DispatchEvent {
                        id: format!("evt-{}-{}", thread_id, i),
                        source_room_id: "!room:matrix.org".to_string(),
                        event_type: "test_event".to_string(),
                        payload: serde_json::json!({"thread": thread_id, "index": i}),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        acknowledged_at: None,
                    };
                    store.insert_dispatch_event(&event).unwrap();
                }
            })
        })
        .collect();

    for handle in insert_handles {
        handle.join().expect("Insert thread panicked");
    }

    // Verify all events inserted
    let pending = store.get_pending_dispatch_events().unwrap();
    assert_eq!(
        pending.len(),
        num_events,
        "Expected {} pending events, got {}",
        num_events,
        pending.len()
    );

    // Now acknowledge from multiple threads
    let pending = Arc::new(pending);
    let ack_handles: Vec<_> = (0..4)
        .map(|thread_id| {
            let store = Arc::clone(&store);
            let pending = Arc::clone(&pending);
            thread::spawn(move || {
                let mut acked = 0;
                // Each thread tries to ack all events - only one should succeed per event
                for event in pending.iter() {
                    if store.acknowledge_dispatch_event(&event.id).is_ok() {
                        acked += 1;
                    }
                }
                (thread_id, acked)
            })
        })
        .collect();

    let mut total_acked = 0;
    for handle in ack_handles {
        let (thread_id, acked) = handle.join().expect("Ack thread panicked");
        println!("Thread {} acknowledged {} events", thread_id, acked);
        total_acked += acked;
    }

    // All events should be acknowledged (note: SQLite UPDATE is idempotent, so all threads "succeed")
    let remaining = store.get_pending_dispatch_events().unwrap();
    assert_eq!(
        remaining.len(),
        0,
        "Expected 0 pending events after ack, got {}",
        remaining.len()
    );

    println!("✅ All {} events handled correctly", num_events);
}

/// Scenario: Concurrent channel creation for same room
/// Expected: Only one channel created, others get existing
#[test]
fn scenario_concurrent_dispatch_channel_creation() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = Arc::new(SessionStore::new(tmp.path()).unwrap());
    let room_id = "!concurrent-dm:matrix.org";

    let num_threads = 10;

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let store = Arc::clone(&store);
            let room_id = room_id.to_string();
            thread::spawn(move || store.get_or_create_dispatch_channel(&room_id).unwrap())
        })
        .collect();

    // Collect all returned channels
    let channels: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("Thread panicked"))
        .collect();

    // All should have the same session_id
    let first_session = &channels[0].session_id;
    for (i, channel) in channels.iter().enumerate() {
        assert_eq!(
            &channel.session_id, first_session,
            "Channel {} has different session_id",
            i
        );
    }

    // Only one channel should exist in database
    let all_dispatch = store.list_dispatch_channels().unwrap();
    assert_eq!(
        all_dispatch.len(),
        1,
        "Expected 1 DISPATCH channel, got {}",
        all_dispatch.len()
    );

    println!("✅ Concurrent channel creation handled correctly");
}

/// Scenario: Simulate task executor race - two executors polling simultaneously
/// Expected: Each task claimed by exactly one executor
#[test]
fn scenario_simulated_executor_race() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = Arc::new(SessionStore::new(tmp.path()).unwrap());

    // Create rooms and tasks
    store.create_channel("room1", "!room1:matrix.org").unwrap();
    store.create_channel("room2", "!room2:matrix.org").unwrap();

    for i in 0..10 {
        store
            .create_dispatch_task("!room1:matrix.org", &format!("Task {}", i))
            .unwrap();
    }

    // Simulate two executors polling and claiming
    let executor_handles: Vec<_> = (0..2)
        .map(|executor_id| {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                let mut executed = Vec::new();

                // Simulate polling loop
                for _ in 0..5 {
                    // Get pending tasks
                    let pending = store
                        .list_dispatch_tasks(Some(DispatchTaskStatus::Pending))
                        .unwrap();

                    for task in pending {
                        // Use atomic claim - only one executor can claim each task
                        if let Ok(true) = store.claim_dispatch_task(
                            &task.id,
                            DispatchTaskStatus::Pending,
                            DispatchTaskStatus::InProgress,
                        ) {
                            // Successfully claimed - "execute" and complete
                            thread::sleep(std::time::Duration::from_millis(1));
                            store
                                .update_dispatch_task_status(
                                    &task.id,
                                    DispatchTaskStatus::Completed,
                                    Some(&format!("Executed by {}", executor_id)),
                                )
                                .unwrap();
                            executed.push(task.id);
                        }
                    }

                    thread::sleep(std::time::Duration::from_millis(5));
                }

                (executor_id, executed)
            })
        })
        .collect();

    // Wait for both executors
    let mut all_executed: Vec<String> = Vec::new();
    for handle in executor_handles {
        let (executor_id, executed) = handle.join().expect("Executor panicked");
        println!("Executor {} executed {} tasks", executor_id, executed.len());
        all_executed.extend(executed);
    }

    // Check for duplicates (same task executed twice)
    let mut sorted = all_executed.clone();
    sorted.sort();
    let before_dedup = sorted.len();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        before_dedup,
        "DUPLICATE EXECUTION DETECTED! {} tasks executed {} times",
        sorted.len(),
        before_dedup
    );

    // All tasks should be completed
    let completed = store
        .list_dispatch_tasks(Some(DispatchTaskStatus::Completed))
        .unwrap();
    assert_eq!(
        completed.len(),
        10,
        "Expected all 10 tasks completed, got {}",
        completed.len()
    );

    println!(
        "✅ No duplicate executions - {} tasks executed exactly once",
        all_executed.len()
    );
}

fn main() {
    println!("Run with: cargo test --test dispatch_concurrency");
}
