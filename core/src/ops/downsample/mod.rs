use crate::internal::*;
use crate::ops;
use ndarray::prelude::*;

mod array;
mod conv;
mod scan;

#[derive(Debug, Clone, new, Default, PartialEq, Hash)]
pub struct Downsample {
    pub axis: usize,
    pub stride: usize,
    pub modulo: usize,
}

impl Downsample {
    fn eval_t<T: Datum>(&self, input: &Tensor) -> TractResult<Arc<Tensor>> {
        let input = input.to_array_view::<T>()?;
        let sampled = if self.modulo < input.shape()[self.axis] {
            input
                .slice_axis(
                    Axis(self.axis),
                    ndarray::Slice::new(self.modulo as isize, None, self.stride as isize),
                )
                .to_owned()
                .into_arc_tensor()
        } else {
            let mut shape = input.shape().to_vec();
            shape[self.axis] = 0;
            unsafe { Tensor::uninitialized::<T>(&shape)?.into_arc_tensor() }
        };
        Ok(sampled)
    }

    pub(crate) fn transform_dim(&self, input_dim: &TDim) -> TDim {
        (input_dim.clone() - self.modulo).div_ceil(self.stride as u32)
    }

    pub(crate) fn transform_fact(&self, input_fact: &TypedFact) -> TractResult<TypedFact> {
        let mut downed = input_fact.clone();
        let down_len = self.transform_dim(&input_fact.shape.dim(self.axis));
        downed.shape.set_dim(self.axis, down_len.clone())?;
        Ok(downed)
    }
}

tract_linalg::impl_dyn_hash!(Downsample);

impl Op for Downsample {
    fn name(&self) -> Cow<str> {
        "Downsample".into()
    }

    fn info(&self) -> TractResult<Vec<String>> {
        Ok(vec![format!("axis:{} stride:{} modulo:{}", self.axis, self.stride, self.modulo)])
    }

    op_core_mir!();
    impl_op_same_as!();
    op_as_typed_op!();
    op_as_pulsed_op!();
}

impl StatelessOp for Downsample {
    fn eval(&self, mut inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let input = args_1!(inputs);
        Ok(tvec!(dispatch_datum!(Self::eval_t(input.datum_type())(self, &*input))?))
    }
}

impl TypedOp for Downsample {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        let mut downed = inputs[0].clone();
        let down_len = self.transform_dim(&downed.shape.dim(self.axis));
        downed.shape.set_dim(self.axis, down_len.clone())?;
        Ok(tvec!(downed))
    }

    fn declutter(
        &self,
        model: &TypedModel,
        node: &TypedNode,
    ) -> TractResult<Option<TypedModelPatch>> {
        if self.stride == 1 {
            return Ok(Some(TypedModelPatch::shunt_one_op(model, node)?));
        }
        pull_downsample_up(model, node)
    }

    fn pulsify(
        &self,
        _source: &NormalizedModel,
        node: &NormalizedNode,
        target: &mut PulsedModel,
        mapping: &HashMap<OutletId, OutletId>,
        _pulse: usize,
    ) -> TractResult<TVec<OutletId>> {
        let input = mapping[&node.inputs[0]];
        let pulse = target.outlet_fact(input)?.pulse();
        if pulse % self.stride != 0 {
            bail!("Pulsificaton requires pulse to be a stride multiple")
        }
        target.wire_node(&*node.name, self.clone(), &[input])
    }

    as_op!();
}

impl PulsedOp for Downsample {
    fn pulsed_output_facts(&self, inputs: &[&PulsedFact]) -> TractResult<TVec<PulsedFact>> {
        let mut fact = inputs[0].clone();
        fact.shape[self.axis] /= self.stride;
        fact.dim = fact.dim.div_ceil(self.stride as u32);
        Ok(tvec!(fact))
    }

    as_op!();
    pulsed_op_to_typed_op!();
}

fn pull_downsample_up(
    model: &TypedModel,
    down_node: &TypedNode,
) -> TractResult<Option<TypedModelPatch>> {
    let down_op = down_node.op_as::<Downsample>().unwrap();
    if let Some(prec) = model.single_prec(down_node.id)? {
        let invariants = prec.op.invariants(model, prec)?;
        debug!("Consider pull {:?} over {:?} (invariants: {:?})", down_op, prec, invariants);
        if let Some(crop_op) = prec.op_as::<ops::array::Slice<TDim>>() {
            return array::pull_downsample_over_slice(model, prec, crop_op, down_node, down_op);
        } else if let Some(crop_op) = prec.op_as::<ops::array::Slice<usize>>() {
            return array::pull_downsample_over_slice(model, prec, crop_op, down_node, down_op);
        } else if let Some(other_op) = prec.op_as::<AxisOp>() {
            return array::pull_downsample_over_axis_op(model, prec, other_op, down_node, down_op);
        } else if let Some(conv_op) = prec.op_as::<ops::cnn::conv::ConvUnary>() {
            return conv::fuse_downsample_into_conv(model, prec, conv_op, down_node, down_op);
        } else if let Some(other_op) = prec.op_as::<ops::scan::Scan>() {
            return scan::pull_downsample_over_scan(model, prec, other_op, down_node, down_op);
        } else if let Some(above_axis) = invariants.unary_track_axis_up(down_op.axis, false) {
            let mut patch = TypedModelPatch::default();
            let mut inputs = vec![];
            for (ix, &oo) in prec.inputs.iter().enumerate() {
                let source = patch.tap_model(model, oo)?;
                let mut op = down_op.clone();
                op.axis = above_axis;
                let ds = patch.wire_node(format!("{}-{}", prec.name, ix), op, [source].as_ref())?;
                inputs.push(ds[0]);
            }
            let other = patch.wire_node(&*prec.name, prec.op.clone(), &*inputs)?;
            patch.shunt_outside(model, OutletId::new(down_node.id, 0), other[0])?;
            return Ok(Some(patch));
        }
    }
    Ok(None)
}
