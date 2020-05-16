#[macro_use]
extern crate criterion;
extern crate tract_linalg;
use criterion::Criterion;

pub fn vec(len: usize, align: usize) -> *mut f32 {
    let layout =
        std::alloc::Layout::from_size_align(len * std::mem::size_of::<f32>(), align).unwrap();
    unsafe { std::alloc::alloc_zeroed(layout) as *mut f32 }
}

fn mat_mul_smmm(be: &mut criterion::Bencher, &(m, k, n): &(usize, usize, usize)) {
    let mm = (tract_linalg::ops().mmm_f32)(m, k, n);
    let pa = vec(mm.a_pack().len(), mm.a_pack().alignment());
    let pb = vec(mm.b_pack().len(), mm.b_pack().alignment());
    let mut c = vec![0.0; m * n];
    be.iter(move || unsafe { mm.run(pa, pb, c.as_mut_ptr(), &[]) });
}

fn mat_mul_prepacked(c: &mut Criterion, m: usize, k: usize, n: usize) {
    c.bench_functions(
        &format!("mat_mul_prepacked"),
        vec![criterion::Fun::new("smmm", mat_mul_smmm)],
        (m, k, n),
    );
}

fn s64x288x21609(c: &mut Criterion) {
    mat_mul_prepacked(c, 64, 288, 21609)
}

criterion_group!(benches, s64x288x21609);
criterion_main!(benches);
