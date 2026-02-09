use super::*;

#[test]
fn test_new_count_executor() {
    let executor = IterativeExecutor::new(IterationCondition::Count { max: 5 });
    assert_eq!(executor.max_iterations, 5);
    assert!(!executor.context_passing_enabled());
}

#[test]
fn test_new_duration_executor() {
    let executor = IterativeExecutor::new(IterationCondition::Duration { max_secs: 60 });
    assert_eq!(executor.max_iterations, 100);
}

#[test]
fn test_new_until_executor() {
    let executor = IterativeExecutor::new(IterationCondition::Until {
        check: "tests pass".to_string(),
    });
    assert_eq!(executor.max_iterations, 50);
}

#[test]
fn test_context_passing_config() {
    let config = ContextPassingConfig {
        cwd: PathBuf::from("/tmp"),
        initial_prompt: "Fix bugs".to_string(),
        plan_content: Some("Plan content".to_string()),
        auto_commit: true,
        enable_complexity_assessment: false,
    };
    let executor = IterativeExecutor::new(IterationCondition::Count { max: 3 })
        .with_context_passing(config);
    assert!(executor.context_passing_enabled());
}

#[tokio::test]
async fn test_execute_count_basic() {
    let mut executor = IterativeExecutor::new(IterationCondition::Count { max: 3 });
    let records = executor.execute("test prompt").await.expect("execute");
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].iteration, 0);
    assert_eq!(records[1].iteration, 1);
    assert_eq!(records[2].iteration, 2);
    // All should succeed with stub
    assert!(records.iter().all(|r| r.success));
}

#[tokio::test]
async fn test_execute_with_simple_callback() {
    let callback: SimpleIterationExecuteFn =
        Arc::new(|i, _prompt| Box::pin(async move { Ok(format!("Result for iteration {i}")) }));

    let mut executor = IterativeExecutor::new(IterationCondition::Count { max: 2 })
        .with_simple_execute_fn(callback);

    let records = executor.execute("test").await.expect("execute");
    assert_eq!(records.len(), 2);
    assert!(records[0].result.contains("iteration 0"));
    assert!(records[1].result.contains("iteration 1"));
}

#[tokio::test]
async fn test_execute_with_full_callback() {
    let callback: IterationExecuteFn = Arc::new(|input| {
        Box::pin(async move {
            Ok(IterationOutput {
                result: format!(
                    "Iteration {} with {} context",
                    input.iteration, input.context.total_iterations
                ),
                success: true,
            })
        })
    });

    let mut executor =
        IterativeExecutor::new(IterationCondition::Count { max: 2 }).with_execute_fn(callback);

    let records = executor.execute("test").await.expect("execute");
    assert_eq!(records.len(), 2);
    assert!(records[0].result.contains("Iteration 0"));
}

#[tokio::test]
async fn test_execute_until_condition() {
    let callback: SimpleIterationExecuteFn = Arc::new(|i, _prompt| {
        Box::pin(async move {
            if i == 2 {
                Ok("tests pass".to_string())
            } else {
                Ok("still working".to_string())
            }
        })
    });

    let mut executor = IterativeExecutor::new(IterationCondition::Until {
        check: "tests pass".to_string(),
    })
    .with_simple_execute_fn(callback);

    let records = executor.execute("test").await.expect("execute");
    assert_eq!(records.len(), 3); // 0, 1, 2 - stops after finding "tests pass"
    assert!(records[2].result.contains("tests pass"));
}

#[tokio::test]
async fn test_progress_callback() {
    use std::sync::atomic::AtomicI32;
    use std::sync::atomic::Ordering;

    let progress_count = Arc::new(AtomicI32::new(0));
    let progress_count_clone = progress_count.clone();

    let mut executor = IterativeExecutor::new(IterationCondition::Count { max: 3 })
        .with_progress_callback(move |_progress| {
            progress_count_clone.fetch_add(1, Ordering::SeqCst);
        });

    let _ = executor.execute("test").await;
    assert_eq!(progress_count.load(Ordering::SeqCst), 3);
}

#[test]
fn test_iteration_input_fields() {
    let input = IterationInput {
        iteration: 5,
        prompt: "Test".to_string(),
        context: IterationContext::new(5, 10),
        cwd: PathBuf::from("/tmp"),
    };
    assert_eq!(input.iteration, 5);
    assert_eq!(input.prompt, "Test");
    assert_eq!(input.context.iteration, 5);
}

#[test]
fn test_iteration_output_fields() {
    let output = IterationOutput {
        result: "Done".to_string(),
        success: true,
    };
    assert_eq!(output.result, "Done");
    assert!(output.success);
}
