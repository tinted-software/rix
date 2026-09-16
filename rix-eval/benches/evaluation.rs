//! Benchmark suite for nix-eval
//!
//! Run benchmarks with: `cargo bench`

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nix_eval::Evaluator;

fn bench_evaluate_integer(c: &mut Criterion) {
    let evaluator = Evaluator::new();
    c.bench_function("evaluate_integer", |b| {
        b.iter(|| evaluator.evaluate(black_box("42")))
    });
}

fn bench_evaluate_string(c: &mut Criterion) {
    let evaluator = Evaluator::new();
    c.bench_function("evaluate_string", |b| {
        b.iter(|| evaluator.evaluate(black_box(r#""hello world""#)))
    });
}

fn bench_evaluate_list(c: &mut Criterion) {
    let evaluator = Evaluator::new();
    c.bench_function("evaluate_list", |b| {
        b.iter(|| evaluator.evaluate(black_box("[1 2 3 4 5]")))
    });
}

fn bench_evaluate_nested_list(c: &mut Criterion) {
    let evaluator = Evaluator::new();
    c.bench_function("evaluate_nested_list", |b| {
        b.iter(|| evaluator.evaluate(black_box("[[1 2] [3 4] [5 6]]")))
    });
}

fn bench_evaluate_attribute_set(c: &mut Criterion) {
    let evaluator = Evaluator::new();
    c.bench_function("evaluate_attribute_set", |b| {
        b.iter(|| evaluator.evaluate(black_box("{ foo = 1; bar = 2; baz = 3; }")))
    });
}

fn bench_evaluate_nested_attribute_set(c: &mut Criterion) {
    let evaluator = Evaluator::new();
    c.bench_function("evaluate_nested_attribute_set", |b| {
        b.iter(|| {
            evaluator.evaluate(black_box(
                "{ outer = { inner = 42; }; another = { value = \"test\"; }; }",
            ))
        })
    });
}

fn bench_evaluate_complex_expression(c: &mut Criterion) {
    let evaluator = Evaluator::new();
    c.bench_function("evaluate_complex_expression", |b| {
        b.iter(|| {
            evaluator.evaluate(black_box(
                r#"{
                    name = "test";
                    items = [1 2 3 4 5];
                    config = { enabled = true; value = 42; };
                }"#,
            ))
        })
    });
}

fn bench_variable_resolution(c: &mut Criterion) {
    use nix_eval::VariableScope;

    let mut evaluator = Evaluator::new();
    let mut scope = VariableScope::new();
    scope.insert("x".to_string(), nix_eval::NixValue::Integer(42));
    scope.insert(
        "y".to_string(),
        nix_eval::NixValue::String("hello".to_string()),
    );
    evaluator.set_scope(scope);

    c.bench_function("variable_resolution", |b| {
        b.iter(|| evaluator.evaluate(black_box("x")))
    });
}

fn bench_large_parallel_collections(c: &mut Criterion) {
    let evaluator = Evaluator::new();
    let list = format!(
        "[ {} ]",
        (1..=128)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );
    c.bench_function("evaluate_large_list", |b| {
        b.iter(|| evaluator.evaluate(black_box(&list)))
    });
}

fn bench_parallel_builtin(c: &mut Criterion) {
    let evaluator = Evaluator::new();
    c.bench_function("parallel_builtin", |b| {
        b.iter(|| evaluator.evaluate(black_box("builtins.parallel [1 2 3] 42")))
    });
}

criterion_group!(
    benches,
    bench_evaluate_integer,
    bench_evaluate_string,
    bench_evaluate_list,
    bench_evaluate_nested_list,
    bench_evaluate_attribute_set,
    bench_evaluate_nested_attribute_set,
    bench_evaluate_complex_expression,
    bench_variable_resolution,
    bench_large_parallel_collections,
    bench_parallel_builtin
);
criterion_main!(benches);
