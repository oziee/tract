use std::fmt;
use std::fmt::Debug;

use num_traits::Zero;

use super::MatMatMulKer;

#[derive(PartialEq, Clone)]
pub enum FusedSpec<TI: Copy + Debug> {
    Min(TI),
    Max(TI),
    AddC,
    PerRowMul(Vec<TI>),
    PerRowAdd(Vec<TI>),
    PerColMul(Vec<TI>),
    PerColAdd(Vec<TI>),
    AddRowColProducts(Vec<TI>, Vec<TI>),
    ScalarMul(TI),
    ScalarAdd(TI),
    QEven(TI, usize),
    QToPlusInf(TI, usize),
}

impl<TI: Copy + Debug> Debug for FusedSpec<TI> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FusedSpec::Min(t) => write!(fmt, "Min({:?})", t),
            FusedSpec::Max(t) => write!(fmt, "Max({:?})", t),
            FusedSpec::AddC => write!(fmt, "AddC"),
            FusedSpec::PerRowMul(_) => write!(fmt, "PerRowMul"),
            FusedSpec::PerRowAdd(_) => write!(fmt, "PerRowAdd"),
            FusedSpec::PerColMul(_) => write!(fmt, "PerColMul"),
            FusedSpec::PerColAdd(_) => write!(fmt, "PerColAdd"),
            FusedSpec::AddRowColProducts(_, _) => write!(fmt, "AddRowColProducts"),
            FusedSpec::ScalarMul(_) => write!(fmt, "ScalarMul"),
            FusedSpec::ScalarAdd(_) => write!(fmt, "ScalarAdd"),
            FusedSpec::QEven(_, _) => write!(fmt, "QEven"),
            FusedSpec::QToPlusInf(_, _) => write!(fmt, "QToPlusInf"),
        }
    }
}

#[repr(C, usize)]
#[derive(PartialEq, Copy, Clone, Debug)]
pub enum FusedKerSpec<TI: Copy> {
    Done,
    Min(TI),
    Max(TI),
    AddC,
    PerRowMul(*const TI),
    PerRowAdd(*const TI),
    PerColMul(*const TI),
    PerColAdd(*const TI),
    AddRowColProducts(*const TI, *const TI),
    ScalarMul(TI),
    ScalarAdd(TI),
    QEven(TI, usize),
    QToPlusInf(TI, usize),
}

pub struct ScratchSpaceFusedNonLinear<TI: Copy> {
    uspecs: Vec<FusedKerSpec<TI>>,
    non_linear_buffers: Vec<Vec<TI>>,
}

impl<TI: Copy> Default for ScratchSpaceFusedNonLinear<TI> {
    fn default() -> ScratchSpaceFusedNonLinear<TI> {
        ScratchSpaceFusedNonLinear { uspecs: vec![], non_linear_buffers: vec![] }
    }
}

impl<TI: Copy> ScratchSpaceFusedNonLinear<TI> {
    pub unsafe fn for_tile<TA, TB, TC, K: MatMatMulKer<TA, TB, TC, TI>>(
        &mut self,
        specs: &[FusedSpec<TI>],
        down: usize,
        right: usize,
    ) -> *const FusedKerSpec<TI>
    where
        TA: Copy,
        TB: Copy,
        TC: Copy + Debug,
        TI: Copy + Debug + Zero,
    {
        self.uspecs.clear();
        for spec in specs {
            let s = match spec {
                FusedSpec::Min(m) => FusedKerSpec::Min(*m),
                FusedSpec::Max(m) => FusedKerSpec::Max(*m),
                FusedSpec::AddC => FusedKerSpec::AddC,
                FusedSpec::PerRowMul(v) => {
                    let have = v.len() - down * K::mr();
                    let ptr = if have < K::mr() {
                        let mut buf = vec![TI::zero(); K::mr()];
                        buf[..have].copy_from_slice(&v[down * K::mr()..][..have]);
                        let ptr = buf.as_ptr();
                        self.non_linear_buffers.push(buf);
                        ptr
                    } else {
                        v.as_ptr().add(down * K::mr())
                    };
                    FusedKerSpec::PerRowMul(ptr)
                }
                FusedSpec::PerRowAdd(v) => {
                    let have = v.len() - down * K::mr();
                    let ptr = if have < K::mr() {
                        let mut buf = vec![TI::zero(); K::mr()];
                        buf[..have].copy_from_slice(&v[down * K::mr()..][..have]);
                        let ptr = buf.as_ptr();
                        self.non_linear_buffers.push(buf);
                        ptr
                    } else {
                        v.as_ptr().add(down * K::mr())
                    };
                    FusedKerSpec::PerRowAdd(ptr)
                }
                FusedSpec::PerColMul(v) => {
                    let have = v.len() - right * K::nr();
                    let ptr = if have < K::nr() {
                        let mut buf = vec![TI::zero(); K::nr()];
                        buf[..have].copy_from_slice(&v[right * K::nr()..][..have]);
                        let ptr = buf.as_ptr();
                        self.non_linear_buffers.push(buf);
                        ptr
                    } else {
                        v.as_ptr().add(right * K::nr())
                    };
                    FusedKerSpec::PerColMul(ptr)
                }
                FusedSpec::PerColAdd(v) => {
                    let have = v.len() - right * K::nr();
                    let ptr = if have < K::nr() {
                        let mut buf = vec![TI::zero(); K::nr()];
                        buf[..have].copy_from_slice(&v[right * K::nr()..][..have]);
                        let ptr = buf.as_ptr();
                        self.non_linear_buffers.push(buf);
                        ptr
                    } else {
                        v.as_ptr().add(right * K::nr())
                    };
                    FusedKerSpec::PerColAdd(ptr)
                }
                FusedSpec::AddRowColProducts(rows, cols) => {
                    let have = rows.len() - down * K::mr();
                    let row_ptr = if have < K::mr() {
                        let mut buf = vec![TI::zero(); K::mr()];
                        buf[..have].copy_from_slice(&rows[down * K::mr()..][..have]);
                        let ptr = buf.as_ptr();
                        self.non_linear_buffers.push(buf);
                        ptr
                    } else {
                        rows.as_ptr().add(down * K::mr())
                    };
                    let have = cols.len() - right * K::nr();
                    let col_ptr = if have < K::nr() {
                        let mut buf = vec![TI::zero(); K::nr()];
                        buf[..have].copy_from_slice(&cols[right * K::nr()..][..have]);
                        let ptr = buf.as_ptr();
                        self.non_linear_buffers.push(buf);
                        ptr
                    } else {
                        cols.as_ptr().add(right * K::nr())
                    };
                    FusedKerSpec::AddRowColProducts(row_ptr, col_ptr)
                }
                FusedSpec::ScalarMul(t) => FusedKerSpec::ScalarMul(*t),
                FusedSpec::ScalarAdd(t) => FusedKerSpec::ScalarAdd(*t),
                FusedSpec::QEven(m, s) => FusedKerSpec::QEven(*m, *s),
                FusedSpec::QToPlusInf(m, s) => FusedKerSpec::QToPlusInf(*m, *s),
            };
            self.uspecs.push(s);
        }
        self.uspecs.push(FusedKerSpec::Done);
        self.uspecs.as_ptr()
    }
}

#[cfg(test)]
#[macro_use]
pub mod test {
    use super::*;
    use crate::frame::mmm::*;
    use crate::frame::mmm::storage::*;
    use num_traits::{AsPrimitive, Bounded, Zero};
    use std::fmt;
    use std::ops::{Add, Mul};

    #[test]
    fn check_non_linear_enum_size() {
        assert_eq!(
            std::mem::size_of::<super::FusedKerSpec<f32>>(),
            3 * std::mem::size_of::<usize>()
        )
    }

    #[macro_export]
    macro_rules! mmm_kernel_fuse_tests {
        ($cond:expr, $ker:ty, $ta:ty, $tb:ty, $tc:ty, $ti: ty) => {
            mod fuse {
                #[allow(unused_imports)]
                use crate::frame::mmm::fuse::test;

                #[test]
                fn return_zeros() {
                    if $cond {
                        test::return_zeros::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }

                #[test]
                fn return_c() {
                    if $cond {
                        test::return_c::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }

                #[test]
                fn return_c_mul_row() {
                    if $cond {
                        test::return_c_mul_row::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }

                #[test]
                fn return_c_add_row() {
                    if $cond {
                        test::return_c_add_row::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }

                #[test]
                fn return_c_mul_col() {
                    if $cond {
                        test::return_c_mul_col::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }

                #[test]
                fn return_c_add_col() {
                    if $cond {
                        test::return_c_add_col::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }

                #[test]
                fn return_c_add_row_col_product() {
                    if $cond {
                        test::return_c_add_row_col_product::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }

                #[test]
                fn return_c_scalar_mul() {
                    if $cond {
                        test::return_c_scalar_mul::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }

                #[test]
                fn return_c_scalar_add() {
                    if $cond {
                        test::return_c_scalar_add::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }
            }
        }
    }


    #[macro_export]
    macro_rules! qmmm_kernel_fuse_tests {
        ($cond:expr, $ker:ty, $ta:ty, $tb:ty, $tc:ty, $ti: ty) => {
            mod kernelq {
                #[allow(unused_imports)]
                use crate::frame::mmm::kernel::test;

                /*
                #[test]
                fn return_c_right_shift_ties_to_even() {
                    if $cond {
                        test::return_c_right_shift_ties_to_even::<$ker, $ta, $tb, $tc, $ti>()
                    }
                }
                */

            }
        }
    }

    pub fn null_packed_storage<T: Copy>() -> PanelStore<T> {
        PanelStore::Packed { ptr: std::ptr::null::<T>() as _ }
    }


    pub fn mmm_stride_storage<T: Copy>(v: &mut [T], rsc: usize) -> PanelStore<T> {
        PanelStore::Strides {
            ptr: v.as_mut_ptr(),
            row_byte_stride: (std::mem::size_of::<T>() * rsc) as isize,
            col_byte_stride: std::mem::size_of::<T>() as isize,
        }
    }

    pub fn return_zeros<K, TA, TB, TC, TI>()
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + Bounded + Zero,
        TI: Copy + Debug,
    {
        let mut v = vec![TC::max_value(); K::mr() * K::nr()];
        let mut c = mmm_stride_storage(&mut v, K::nr());
        let err = K::kernel(&MatMatMulKerSpec {
            a: &null_packed_storage(),
            b: &null_packed_storage(),
            c: &mut c,
            linear: &LinearSpec::k(0),
            non_linear: std::ptr::null(),
        });
        assert_eq!(err, 0);
        assert!(v.iter().all(|&a| a.is_zero()));
    }

    pub fn fused_ops<K, TA, TB, TC, TI>(c: &[TC], ops: &[FusedKerSpec<TI>]) -> Vec<TC>
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + 'static + PartialEq,
        TI: Copy + Debug,
        usize: AsPrimitive<TC>,
    {
        assert!(c.len() == K::mr() * K::nr());
        let mut v = c.to_vec();
        let mut c = mmm_stride_storage(&mut v, K::nr());
        let mut ops = ops.to_vec();
        ops.insert(0, FusedKerSpec::AddC);
        ops.push(FusedKerSpec::Done);
        let err = K::kernel(&MatMatMulKerSpec {
            a: &null_packed_storage(),
            b: &null_packed_storage(),
            c: &mut c,
            linear: &LinearSpec::k(0),
            non_linear: ops.as_ptr(),
        });
        assert_eq!(err, 0);
        v
    }

    pub fn return_c<K, TA, TB, TC, TI>()
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + Debug + 'static + PartialEq,
        TI: Copy + Debug,
        usize: AsPrimitive<TC>,
    {
        let len = K::mr() * K::nr();
        let v: Vec<TC> = (0..len).map(|f| f.as_()).collect();
        let found = fused_ops::<K, TA, TB, TC, TI>(&*v, &[]);
        assert_eq!(found, v);
    }

    pub fn return_c_mul_row<K, TA, TB, TC, TI>()
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + 'static + PartialEq,
        TI: Copy + Add + Mul<Output = TI> + Zero + Debug + fmt::Display + 'static + AsPrimitive<TC>,
        usize: AsPrimitive<TC> + AsPrimitive<TI>,
    {
        let len = K::mr() * K::nr();
        let v: Vec<TC> = (0..len).map(|f| f.as_()).collect();
        let bias: Vec<TI> = (0..K::mr()).map(|f| f.as_()).collect();
        let found = fused_ops::<K, TA, TB, TC, TI>(&*v, &[FusedKerSpec::PerRowMul(bias.as_ptr())]);
        assert!(found.iter().enumerate().all(|(ix, &a)| {
            let row = ix / K::nr();
            let ix: TI = ix.as_();
            a == (ix * bias[row]).as_()
        }));
    }

    pub fn return_c_add_row<K, TA, TB, TC, TI>()
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + PartialEq + 'static,
        TI: Copy + Add + Mul + Zero + Debug + fmt::Display + PartialEq + 'static + AsPrimitive<TC>,
        usize: AsPrimitive<TC> + AsPrimitive<TI>,
    {
        let len = K::mr() * K::nr();
        let v: Vec<TC> = (0..len).map(|f| f.as_()).collect();
        let bias: Vec<TI> = (0..K::mr()).map(|f| f.as_()).collect();
        let found = fused_ops::<K, TA, TB, TC, TI>(&*v, &[FusedKerSpec::PerRowAdd(bias.as_ptr())]);
        assert!(found.iter().enumerate().all(|(ix, &a)| {
            let row = ix / K::nr();
            let ix: TI = ix.as_();
            a == (ix + bias[row]).as_()
        }));
    }

    pub fn return_c_mul_col<K, TA, TB, TC, TI>()
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + 'static + PartialEq,
        TI: Copy + Add + Mul<Output = TI> + Zero + Debug + fmt::Display + 'static + AsPrimitive<TC>,
        usize: AsPrimitive<TC> + AsPrimitive<TI>,
    {
        let len = K::mr() * K::nr();
        let v: Vec<TC> = (0..len).map(|f| f.as_()).collect();
        let bias: Vec<TI> = (0..K::nr()).map(|f| f.as_()).collect();
        let found = fused_ops::<K, TA, TB, TC, TI>(&*v, &[FusedKerSpec::PerColMul(bias.as_ptr())]);
        assert!(found.iter().enumerate().all(|(ix, &a)| {
            let col = ix % K::nr();
            let ix: TI = ix.as_();
            a == (ix * bias[col]).as_()
        }));
    }

    pub fn return_c_add_col<K, TA, TB, TC, TI>()
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + PartialEq + 'static,
        TI: Copy + Add + Mul + Zero + Debug + fmt::Display + PartialEq + 'static + AsPrimitive<TC>,
        usize: AsPrimitive<TC> + AsPrimitive<TI>,
    {
        let len = K::mr() * K::nr();
        let v: Vec<TC> = (0..len).map(|f| f.as_()).collect();
        let bias: Vec<TI> = (0..K::nr()).map(|f| f.as_()).collect();
        let found = fused_ops::<K, TA, TB, TC, TI>(&*v, &[FusedKerSpec::PerColAdd(bias.as_ptr())]);
        assert!(found.iter().enumerate().all(|(ix, &a)| {
            let col = ix % K::nr();
            let ix: TI = ix.as_();
            a == (ix + bias[col]).as_()
        }));
    }

    pub fn return_c_add_row_col_product<K, TA, TB, TC, TI>()
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + PartialEq + 'static,
        TI: Copy
            + Add
            + Mul<Output = TI>
            + Zero
            + Debug
            + fmt::Display
            + PartialEq
            + 'static
            + AsPrimitive<TC>,
        usize: AsPrimitive<TC> + AsPrimitive<TI>,
    {
        let len = K::mr() * K::nr();
        let v: Vec<TC> = (0..len).map(|f| f.as_()).collect();
        let rows: Vec<TI> = (0..K::mr()).map(|f| f.as_()).collect();
        let cols: Vec<TI> = (0..K::nr()).map(|f| f.as_()).collect();
        let found = fused_ops::<K, TA, TB, TC, TI>(
            &*v,
            &[FusedKerSpec::AddRowColProducts(rows.as_ptr(), cols.as_ptr())],
        );
        assert!(found.iter().enumerate().all(|(ix, &a)| {
            let row = ix / K::nr();
            let col = ix % K::nr();
            let ix: TI = ix.as_();
            a == (ix + cols[col] * rows[row]).as_()
        }));
    }

    pub fn return_c_scalar_mul<K, TA, TB, TC, TI>()
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + PartialEq + 'static + Debug,
        TI: Copy
            + Add
            + Mul<Output = TI>
            + Zero
            + Debug
            + fmt::Display
            + PartialEq
            + 'static
            + AsPrimitive<TC>,
        usize: AsPrimitive<TC> + AsPrimitive<TI>,
    {
        let len = K::mr() * K::nr();
        let v: Vec<TC> = (0..len).map(|f| f.as_()).collect();
        let found = fused_ops::<K, TA, TB, TC, TI>(&*v, &[FusedKerSpec::ScalarMul(5.as_())]);
        assert!(found.iter().enumerate().all(|(ix, &a)| {
            let ix: TI = ix.as_();
            a == (ix * 5.as_()).as_()
        }));
    }

    pub fn return_c_scalar_add<K, TA, TB, TC, TI>()
    where
        K: MatMatMulKer<TA, TB, TC, TI>,
        TA: Copy,
        TB: Copy,
        TC: Copy + PartialEq + 'static,
        TI: Copy
            + Add
            + Mul<Output = TI>
            + Zero
            + Debug
            + fmt::Display
            + PartialEq
            + 'static
            + AsPrimitive<TC>,
        usize: AsPrimitive<TC> + AsPrimitive<TI>,
    {
        let len = K::mr() * K::nr();
        let v: Vec<TC> = (0..len).map(|f| f.as_()).collect();
        let found = fused_ops::<K, TA, TB, TC, TI>(&*v, &[FusedKerSpec::ScalarAdd(5.as_())]);
        assert!(found.iter().enumerate().all(|(ix, &a)| {
            let ix: TI = ix.as_();
            a == (ix + 5.as_()).as_()
        }));
    }


}
