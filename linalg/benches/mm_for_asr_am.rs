use criterion::*;

pub fn vec<T>(len: usize, align: usize) -> *mut T {
    let layout =
        std::alloc::Layout::from_size_align(len * std::mem::size_of::<T>(), align).unwrap();
    unsafe { std::alloc::alloc_zeroed(layout) as *mut T }
}

fn mat_mul_f32(be: &mut Bencher, &(m, k, n): &(usize, usize, usize)) {
    let mm = (tract_linalg::ops().mmm_f32)(m, k, n);
    let pa = vec(mm.a_pack().len(), mm.a_pack().alignment());
    let pb = vec(mm.b_pack().len(), mm.b_pack().alignment());
    let mut c = vec![0.0; m * n];
    be.iter(move || unsafe { mm.run(pa, pb, c.as_mut_ptr(), &[]) });
}

fn mat_mul_i8(be: &mut criterion::Bencher, &(m, k, n): &(usize, usize, usize)) {
    let mm = (tract_linalg::ops().qmmm_i8_i8)(m, k, n);
    let pa = vec(mm.as_mmm().a_pack().len(), mm.as_mmm().a_pack().alignment());
    let pb = vec(mm.as_mmm().b_pack().len(), mm.as_mmm().b_pack().alignment());
    let mut c = vec![0i8; m * n];
    be.iter(move || unsafe { mm.run(pa, pb, c.as_mut_ptr(), &[]) });
}

fn packed_packed(c: &mut Criterion, m: usize, k: usize, n: usize) {
    let mut group = c.benchmark_group("packed_packed");
    let id = format!("{}x{}x{}", m, k, n);
    group.bench_with_input(BenchmarkId::new("f32", &id), &(m, k, n), mat_mul_f32);
    group.bench_with_input(BenchmarkId::new("i8", &id), &(m, k, n), mat_mul_i8);
}

type ConvGeo = (usize, usize, usize, usize, usize);

fn direct_conv_geo(
    &(pulse, kern, ci, co, stride): &ConvGeo,
) -> (usize, usize, usize, Vec<isize>, Vec<isize>, usize) {
    let (m, k, n) = (co, kern * ci, pulse / stride);
    let rows_offsets: Vec<isize> =
        (0..ci).flat_map(move |ici| (0..kern).map(move |ik| (ik * ci + ici) as isize)).collect();
    let cols_offsets: Vec<isize> = (0..n).map(move |i| (i * ci * stride) as isize).collect();
    let b_len = cols_offsets.iter().max().unwrap() + rows_offsets.iter().max().unwrap() + 1;
    (m, k, n, rows_offsets, cols_offsets, b_len as usize)
}

fn direct_conv_mmm_f32(be: &mut Bencher, geo: &ConvGeo) {
    let (m, k, n, rows_offsets, cols_offsets, b_len) = direct_conv_geo(geo);
    let mm = (tract_linalg::ops().mmm_f32)(m, k, n);
    let pa = vec(mm.a_pack().len(), mm.a_pack().alignment());
    let pb = vec![0.0; b_len];
    let mut c = vec![0.0; m * n];
    let mut mm = (tract_linalg::ops().mmm_f32)(m, k, n);
    unsafe {
        mm.b_from_data_and_offsets(&rows_offsets, &cols_offsets);
    }
    be.iter(move || unsafe { mm.run(pa, pb.as_ptr(), c.as_mut_ptr(), &[]) });
}

fn direct_conv_i8(be: &mut Bencher, geo: &ConvGeo) {
    let (m, k, n, rows_offsets, cols_offsets, b_len) = direct_conv_geo(geo);
    let mm = (tract_linalg::ops().mmm_f32)(m, k, n);
    let pa = vec(mm.a_pack().len(), mm.a_pack().alignment());
    let pb = vec![0; b_len];
    let mut c = vec![0; m * n];
    let mut mm = (tract_linalg::ops().qmmm_i8_i8)(m, k, n);
    unsafe {
        mm.as_mmm_mut().b_from_data_and_offsets(&rows_offsets, &cols_offsets);
    }
    be.iter(move || unsafe { mm.run(pa, pb.as_ptr(), c.as_mut_ptr(), &[]) });
}

fn direct_conv(c: &mut Criterion, p: usize, kl: usize, ci: usize, co: usize, stride: usize) {
    let mut group = c.benchmark_group("conv");
    let id = format!("{}x{}x{}x{}", p, kl, ci, co);
    group.bench_with_input(
        BenchmarkId::new("f32", &id),
        &(p, kl, ci, co, stride),
        direct_conv_mmm_f32,
    );
    group.bench_with_input(BenchmarkId::new("i8", &id), &(p, kl, ci, co, stride), direct_conv_i8);
}

fn all(c: &mut Criterion) {
    direct_conv(c, 24, 5, 40, 200, 1); // lda
    packed_packed(c, 256, 200, 24); // tdnn1
    direct_conv(c, 24, 3, 256, 256, 1); // tdnn2
    direct_conv(c, 24, 3, 256, 256, 3); // tdnn3
    packed_packed(c, 256, 256, 8); // fastlstm1 and 2 (input) x 8 (4 prod x 2 layers)
    packed_packed(c, 256, 128, 1); // fastlstm1 and 2 (hidden) x 64 (4 prod x 2 layers x 8 loops)
    packed_packed(c, 256, 256, 1); // fastlstm1 and 2 (rp) x 16 (2 layers x 8 loops)
    direct_conv(c, 8, 3, 256, 256, 1); // tdnn4, tdd5 (x2)
    packed_packed(c, 1690, 256, 8); // output
}

criterion_group!(benches, all);
criterion_main!(benches);
