use criterion::{Criterion, Throughput, criterion_group, criterion_main};

fn all_packages(c: &mut Criterion) {
    let input = include_str!("all-packages.nix");
    let mut group = c.benchmark_group("all-packages");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.sample_size(30);
    group.bench_with_input("all-packages", input, move |b, input| {
        b.iter(|| rix_parser::Root::parse(input))
    });
    group.finish();
}

criterion_group!(benches, all_packages);
criterion_main!(benches);
