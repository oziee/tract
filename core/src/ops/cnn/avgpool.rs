use crate::internal::*;
use num_traits::AsPrimitive;
use std::iter::Sum;

use crate::ops::cnn::pools::PoolSpec;
use crate::ops::cnn::Patch;
use crate::ops::nn::DataShape;

#[derive(Debug, Clone, new, Default, Hash)]
pub struct AvgPool {
    pub pool_spec: PoolSpec,
    pub count_include_pad: bool,
}

impl AvgPool {
    fn to_fixed(
        &self,
        datum_type: DatumType,
        input_shape: &[usize],
    ) -> TractResult<Box<dyn TypedOp>> {
        let (input_shape, patch, output_shape) = self.pool_spec.compute_geo(input_shape)?;
        let op =
            AvgPoolFixed::new(patch, input_shape, output_shape, datum_type, self.count_include_pad);
        Ok(Box::new(op))
    }
}

impl Op for AvgPool {
    fn name(&self) -> Cow<str> {
        "AvgPool".into()
    }

    fn info(&self) -> TractResult<Vec<String>> {
        Ok(self.pool_spec.info())
    }

    fn validation(&self) -> Validation {
        Validation::Rounding
    }

    canonic!();
    op_as_typed_op!();
    op_as_pulsed_op!();
}

tract_linalg::impl_dyn_hash!(AvgPool);

impl StatelessOp for AvgPool {
    fn eval(&self, inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let op = self.to_fixed(inputs[0].datum_type(), inputs[0].shape())?;
        op.as_stateless().unwrap().eval(inputs)
    }
}

impl TypedOp for AvgPool {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        self.pool_spec.output_facts(inputs)
    }

    fn pulsify(
        &self,
        source: &NormalizedModel,
        node: &NormalizedNode,
        target: &mut PulsedModel,
        mapping: &HashMap<OutletId, OutletId>,
        _pulse: usize,
    ) -> TractResult<TVec<OutletId>> {
        self.pool_spec.pulsify(source, node, self, target, mapping)
    }

    fn codegen(
        &self,
        model: &TypedModel,
        node: &TypedNode,
    ) -> TractResult<Option<TypedModelPatch>> {
        let inputs = model.node_input_facts(node.id)?;
        if let Some(shape) = inputs[0].shape.as_finite() {
            let op = self.to_fixed(inputs[0].datum_type, shape)?;
            return Ok(Some(TypedModelPatch::single_unary_op(model, node, op)?));
        }
        Ok(None)
    }

    as_op!();
}

impl PulsedOp for AvgPool {
    fn pulsed_output_facts(&self, inputs: &[&PulsedFact]) -> TractResult<TVec<PulsedFact>> {
        self.pool_spec.pulsed_output_facts(inputs)
    }

    as_op!();
    pulsed_op_to_typed_op!();
}

#[derive(Debug, Clone, new, Hash)]
pub struct AvgPoolFixed {
    patch: Patch,
    input_shape: DataShape,
    output_shape: DataShape,
    datum_type: DatumType,
    count_include_pad: bool,
}

tract_linalg::impl_dyn_hash!(AvgPoolFixed);

impl AvgPoolFixed {
    fn eval_t<T: Copy + Datum + num_traits::Float + Sum>(
        &self,
        input: &Tensor,
        values_ptr: *mut T,
    ) -> TractResult<()>
    where
        usize: AsPrimitive<T>,
    {
        let input_ptr = input.as_ptr::<T>()?;

        let n = *self.input_shape.n().unwrap_or(&1);
        let n_stride_i = self.input_shape.n_stride().unwrap_or(&0);
        let n_stride_o = self.output_shape.n_stride().unwrap_or(&0);
        unsafe {
            self.patch.visit_output(|visitor| {
                let div = if self.count_include_pad {
                    self.patch.standard_layout_data_field.len()
                } else {
                    visitor.valid_count()
                };
                let div = div.as_().recip();
                for n in 0..n {
                    let input_offset = n * n_stride_i;
                    let output_offset = n * n_stride_o;
                    for c in 0..*self.input_shape.c() {
                        let input_offset = input_offset + self.input_shape.c_stride() * c;
                        let output_offset = output_offset + self.output_shape.c_stride() * c;
                        let sum = visitor
                            .valid_offsets()
                            .map(|v| *input_ptr.offset(v + input_offset as isize))
                            .sum::<T>();

                        *values_ptr.offset(output_offset as isize + visitor.output_offset) =
                            sum * div;
                    }
                }
            });
        }
        Ok(())
    }
}

impl Op for AvgPoolFixed {
    fn name(&self) -> Cow<str> {
        "AvgPool::Fixed".into()
    }

    op_as_typed_op!();
    not_a_pulsed_op!();
}

impl StatelessOp for AvgPoolFixed {
    fn eval(&self, mut inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let mut values =
            unsafe { Tensor::uninitialized_dt(self.datum_type, &*self.output_shape.shape)? };
        let input = args_1!(inputs);
        dispatch_floatlike!(Self::eval_t(input.datum_type())(self, &*input, values.as_ptr_mut()?))?;
        Ok(tvec!(values.into_arc_tensor()))
    }
}

impl TypedOp for AvgPoolFixed {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(TypedFact::dt_shape(inputs[0].datum_type, &*self.output_shape.shape)?))
    }

    as_op!();
}
