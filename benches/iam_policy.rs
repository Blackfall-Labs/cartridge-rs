use cartridge::iam::{Action, Effect, Policy, PolicyEngine, Statement};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

/// Create a complex policy with multiple statements
fn create_complex_policy() -> Policy {
    let mut policy = Policy::new();

    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec!["public/**".to_string()],
    ));

    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read, Action::Write],
        vec!["users/*/documents/**".to_string()],
    ));

    policy.add_statement(Statement::new(
        Effect::Deny,
        vec![Action::Write],
        vec!["system/**".to_string()],
    ));

    policy
}

/// Benchmark policy evaluation with cache (hot path)
fn bench_policy_eval_cached(c: &mut Criterion) {
    let eval_counts = vec![100, 1_000, 10_000];

    let mut group = c.benchmark_group("policy_eval_cached");

    for count in eval_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let policy = create_complex_policy();
            let mut engine = PolicyEngine::new_default();

            b.iter(|| {
                // Repeatedly evaluate same path (should hit cache)
                for _ in 0..count {
                    let allowed = engine.evaluate(&policy, &Action::Read, "public/readme.md", None);
                    black_box(allowed);
                }
            });
        });
    }

    group.finish();
}

/// Benchmark policy evaluation without cache (cold path)
fn bench_policy_eval_uncached(c: &mut Criterion) {
    let eval_counts = vec![100, 1_000, 5_000];

    let mut group = c.benchmark_group("policy_eval_uncached");

    for count in eval_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let policy = create_complex_policy();

            b.iter(|| {
                // Create fresh engine each time (no cache)
                let mut engine = PolicyEngine::new_default();
                for i in 0..count {
                    let path = format!("public/file_{}.txt", i);
                    let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                    black_box(allowed);
                }
            });
        });
    }

    group.finish();
}

/// Benchmark cache hit rate measurement
fn bench_cache_hit_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_hit_rate");

    group.bench_function("90_percent_hit_rate", |b| {
        let policy = create_complex_policy();
        let mut engine = PolicyEngine::new_default();

        b.iter(|| {
            // 90% access to same 10 paths (hot set)
            for _ in 0..90 {
                let i = rand::random::<usize>() % 10;
                let path = format!("public/file_{}.txt", i);
                let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                black_box(allowed);
            }

            // 10% access to unique paths (cold set)
            for i in 10..20 {
                let path = format!("public/file_{}.txt", i);
                let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                black_box(allowed);
            }
        });
    });

    group.bench_function("50_percent_hit_rate", |b| {
        let policy = create_complex_policy();
        let mut engine = PolicyEngine::new_default();

        b.iter(|| {
            // 50/50 mix of cached and uncached
            for i in 0..100 {
                let path = if i % 2 == 0 {
                    format!("public/file_{}.txt", i % 10) // Cached
                } else {
                    format!("public/file_{}.txt", i) // Uncached
                };
                let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                black_box(allowed);
            }
        });
    });

    group.finish();
}

/// Benchmark wildcard pattern matching performance
fn bench_wildcard_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("wildcard_matching");

    // Simple wildcard (single *)
    group.bench_function("simple_wildcard", |b| {
        let mut policy = Policy::new();
        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read],
            vec!["users/*/documents".to_string()],
        ));

        let mut engine = PolicyEngine::new_default();

        b.iter(|| {
            for i in 0..100 {
                let path = format!("users/user{}/documents", i);
                let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                black_box(allowed);
            }
        });
    });

    // Complex wildcard (double **)
    group.bench_function("recursive_wildcard", |b| {
        let mut policy = Policy::new();
        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read],
            vec!["users/**".to_string()],
        ));

        let mut engine = PolicyEngine::new_default();

        b.iter(|| {
            for i in 0..100 {
                let path = format!("users/user{}/docs/nested/file.txt", i);
                let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                black_box(allowed);
            }
        });
    });

    // Mixed wildcards
    group.bench_function("mixed_wildcards", |b| {
        let mut policy = Policy::new();
        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read],
            vec!["projects/*/code/**/*.rs".to_string()],
        ));

        let mut engine = PolicyEngine::new_default();

        b.iter(|| {
            for i in 0..100 {
                let path = format!("projects/project{}/code/src/main.rs", i);
                let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                black_box(allowed);
            }
        });
    });

    group.finish();
}

/// Benchmark policy with many statements
fn bench_policy_complexity(c: &mut Criterion) {
    let statement_counts = vec![5, 25, 100];

    let mut group = c.benchmark_group("policy_complexity");

    for count in statement_counts {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            // Create policy with N statements
            let mut policy = Policy::new();
            for i in 0..count {
                policy.add_statement(Statement::new(
                    Effect::Allow,
                    vec![Action::Read],
                    vec![format!("path_{}/**", i)],
                ));
            }

            let mut engine = PolicyEngine::new_default();

            b.iter(|| {
                // Evaluate against policy (may need to check all statements)
                for i in 0..100 {
                    let path = format!("path_{}/file.txt", i % count);
                    let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                    black_box(allowed);
                }
            });
        });
    }

    group.finish();
}

/// Benchmark cache eviction and replacement
fn bench_cache_eviction(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_eviction");

    group.bench_function("lru_eviction", |b| {
        let policy = create_complex_policy();
        let mut engine = PolicyEngine::new_default();

        b.iter(|| {
            // Access more paths than cache can hold (force evictions)
            for i in 0..1000 {
                let path = format!("public/file_{}.txt", i);
                let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                black_box(allowed);
            }
        });
    });

    group.finish();
}

/// Benchmark deny vs allow evaluation
fn bench_deny_vs_allow(c: &mut Criterion) {
    let mut group = c.benchmark_group("deny_vs_allow");

    group.bench_function("allow_match", |b| {
        let mut policy = Policy::new();
        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read],
            vec!["allowed/**".to_string()],
        ));

        let mut engine = PolicyEngine::new_default();

        b.iter(|| {
            for i in 0..100 {
                let path = format!("allowed/file_{}.txt", i);
                let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                black_box(allowed);
            }
        });
    });

    group.bench_function("deny_match", |b| {
        let mut policy = Policy::new();
        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read],
            vec!["**".to_string()],
        ));
        policy.add_statement(Statement::new(
            Effect::Deny,
            vec![Action::Read],
            vec!["denied/**".to_string()],
        ));

        let mut engine = PolicyEngine::new_default();

        b.iter(|| {
            for i in 0..100 {
                let path = format!("denied/file_{}.txt", i);
                let allowed = engine.evaluate(&policy, &Action::Read, &path, None);
                black_box(allowed);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_policy_eval_cached,
    bench_policy_eval_uncached,
    bench_cache_hit_rate,
    bench_wildcard_matching,
    bench_policy_complexity,
    bench_cache_eviction,
    bench_deny_vs_allow,
);
criterion_main!(benches);
