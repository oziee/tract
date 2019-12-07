use crate::internal::*;
use crate::ops::cnn::Patch;
use crate::ops::nn::DataShape;
use ndarray::*;

#[derive(Debug, Clone, new)]
pub struct DepthWise<T>
where
    T: Datum + Clone + ndarray::LinalgScalar,
{
    patch: Patch,
    input_shape: DataShape,
    output_shape: DataShape,
    kernel_chw: ArrayD<T>,
    bias: Option<Vec<T>>,
}

impl<T> Op for DepthWise<T>
where
    T: Datum + Clone + ndarray::LinalgScalar,
{
    fn name(&self) -> Cow<str> {
        format!("Conv::DepthWise<{:?}>", T::datum_type()).into()
    }

    op_as_typed_op!();
    not_a_pulsed_op!();
}

impl<T> StatelessOp for DepthWise<T>
where
    T: Datum + Clone + ndarray::LinalgScalar,
{
    fn eval(&self, mut inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let img = args_1!(inputs);
        let img = img.to_array_view::<T>()?;
        let iptr = img.as_ptr();
        let mut output = unsafe { ArrayD::<T>::uninitialized(&*self.output_shape.shape) };
        let optr = output.as_mut_ptr();
        let k_stride_o = self.kernel_chw.strides()[0];
        let k_stride_i = self.kernel_chw.strides()[1];
        let mult = *self.output_shape.c() / *self.input_shape.c();
        unsafe {
            self.patch.visit_output(|visitor| {
                for n in 0..*self.input_shape.n() {
                    let input_offset = *self.input_shape.n_stride() * n;
                    let output_offset = *self.output_shape.n_stride() * n;
                    for c in 0..*self.input_shape.c() {
                        let input_offset = input_offset + self.input_shape.c_stride() * c;
                        for m in 0..mult {
                            let mut sum = if let Some(b) = &self.bias {
                                b[m + c * mult]
                            } else {
                                T::zero()
                            };
                            let output_offset =
                                output_offset + self.output_shape.c_stride() * (m + c * mult);
                            let kptr = self
                                .kernel_chw
                                .as_ptr()
                                .offset(k_stride_i * c as isize + k_stride_o * m as isize);
                            for (ix, v) in visitor.valid_offsets_with_indexes() {
                                let k = *kptr.offset(ix as isize);
                                let i = *iptr.offset(input_offset as isize + v);
                                sum = sum + k * i;
                            }
                            let ptr = optr.offset(output_offset as isize + visitor.output_offset);
                            *ptr = sum;
                        }
                    }
                }
            });
        }
        Ok(tvec!(output.into_arc_tensor()))
    }
}

impl<T> TypedOp for DepthWise<T>
where
    T: Datum + Clone + ndarray::LinalgScalar,
{
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(TypedFact::dt_shape(inputs[0].datum_type, &*self.output_shape.shape)?))
    }

    fn cost(&self, _inputs: &[&TypedFact]) -> TractResult<TVec<(Cost, TDim)>> {
        let n_output_points = self.patch.output_shape.iter().cloned().product::<usize>();
        Ok(tvec!((
            Cost::FMA(T::datum_type()),
            (self.input_shape.n() * n_output_points * self.kernel_chw.len()).to_dim()
        )))
    }

    typed_op_as_op!();
}
