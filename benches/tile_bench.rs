use criterion::{Criterion, criterion_group, criterion_main};

fn tile_count(width: u32, height: u32, tile: u32) -> u32 {
    let tiles_x = (width + tile - 1) / tile;
    let tiles_y = (height + tile - 1) / tile;
    tiles_x * tiles_y
}

fn bench_tiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("inpaint_tiles");
    for &tile in &[32, 64, 96, 128] {
        group.bench_function(format!("tile_{tile}"), |b| {
            b.iter(|| tile_count(1280, 720, tile))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_tiles);
criterion_main!(benches);
