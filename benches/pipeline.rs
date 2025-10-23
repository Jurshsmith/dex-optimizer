use std::{sync::Arc, time::Duration};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use optimizer::{
    dataset::{self, Dataset},
    pipeline::{self, PipelineConfig},
};

fn load_benchmark_dataset() -> Arc<Dataset> {
    dataset::load_default_dataset()
        .map(Arc::new)
        .expect("Datasets required to run benchmarks")
}

fn pipeline_hop_cap_group(c: &mut Criterion) {
    let dataset = load_benchmark_dataset();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to build benchmark runtime");

    let mut group = c.benchmark_group("pipeline_hop_cap");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(20));
    group.warm_up_time(Duration::from_secs(3));
    let base_config = PipelineConfig {
        max_updates: 512,
        channel_capacity: 128,
        hop_cap: 4,
        search_interval: Duration::from_millis(10),
        coalesce_window: Duration::from_millis(2),
        max_coalesce: 32,
        rate_jitter: 0.02,
        ..PipelineConfig::default()
    };

    for &hop_cap in &[2usize, 4, 6, 8] {
        let dataset = Arc::clone(&dataset);
        let mut config_template = base_config.clone();
        config_template.hop_cap = hop_cap;
        let expected_updates = config_template.max_updates;
        let config = Arc::new(config_template);

        group.throughput(Throughput::Elements(expected_updates as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(hop_cap),
            &hop_cap,
            |b, &_hop| {
                let dataset = Arc::clone(&dataset);
                let config = Arc::clone(&config);
                let expected_updates = expected_updates;
                b.to_async(&runtime).iter(move || {
                    let dataset = Arc::clone(&dataset);
                    let config = Arc::clone(&config);
                    let expected_updates = expected_updates;
                    async move {
                        let stats = pipeline::run((*dataset).clone(), (*config).clone())
                            .await
                            .expect("pipeline benchmark run");
                        assert_eq!(stats.updates_processed, expected_updates);
                        assert!(stats.unique_updates_applied <= stats.updates_processed);
                        std::hint::black_box(stats);
                    }
                });
            },
        );
    }

    group.finish();
}

fn pipeline_throughput_group(c: &mut Criterion) {
    let dataset = load_benchmark_dataset();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to build benchmark runtime");

    let mut group = c.benchmark_group("pipeline_throughput");
    group.sample_size(25);
    group.measurement_time(Duration::from_secs(15));
    group.warm_up_time(Duration::from_secs(2));

    let scenarios = [
        (64usize, 2usize, Duration::from_millis(2)),
        (128, 4, Duration::from_millis(3)),
        (256, 6, Duration::from_millis(4)),
    ];

    for &(max_updates, hop_cap, search_interval) in &scenarios {
        let dataset = Arc::clone(&dataset);
        let config_template = PipelineConfig {
            max_updates,
            hop_cap,
            channel_capacity: 64,
            search_interval,
            coalesce_window: Duration::from_millis(1),
            max_coalesce: 16,
            rate_jitter: 0.01,
            ..PipelineConfig::default()
        };
        let expected_updates = config_template.max_updates;
        let config = Arc::new(config_template);
        let config_id = format!("updates{}_hop{}", max_updates, hop_cap);
        group.throughput(Throughput::Elements(expected_updates as u64));

        group.bench_function(BenchmarkId::new("pipeline", config_id), |b| {
            let dataset = Arc::clone(&dataset);
            let config = Arc::clone(&config);
            let expected_updates = expected_updates;
            b.to_async(&runtime).iter(move || {
                let dataset = Arc::clone(&dataset);
                let config = Arc::clone(&config);
                let expected_updates = expected_updates;
                async move {
                    let stats = pipeline::run((*dataset).clone(), (*config).clone())
                        .await
                        .expect("pipeline throughput run");
                    assert_eq!(stats.updates_processed, expected_updates);
                    assert!(stats.unique_updates_applied <= stats.updates_processed);
                    std::hint::black_box(stats.unique_updates_applied);
                }
            });
        });
    }

    group.finish();
}

criterion_group!(benches, pipeline_hop_cap_group, pipeline_throughput_group);
criterion_main!(benches);
