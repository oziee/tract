use crate::internal::*;
use ndarray::prelude::*;

#[derive(Debug, Clone, new, Default)]
pub struct Slice<D: DimLike + ToDim> {
    pub axis: usize,
    pub start: D,
    pub end: D,
}

impl<D: DimLike + ToDim> Slice<D> {
    fn eval_t<T: Datum>(&self, input: Arc<Tensor>) -> TractResult<Arc<Tensor>> {
        let mut input = input.to_array_view::<T>()?;
        input.slice_axis_inplace(
            Axis(self.axis),
            ::ndarray::Slice::from((self.start.to_integer()?)..(self.end.to_integer()?)),
        );
        if self.start == self.end {
            // dodge a bug in ndarray :/
            unsafe { return Ok(Tensor::from_raw::<T>(input.shape(), &[])?.into()) }
        }
        Ok(Tensor::from(input.to_owned()).into())
    }
}

impl<D: DimLike + ToDim> Op for Slice<D> {
    fn name(&self) -> Cow<str> {
        "Slice".into()
    }

    fn info(&self) -> TractResult<Vec<String>> {
        Ok(vec![format!("axis: {}, {}..{}", self.axis, self.start, self.end)])
    }

    canonic!();
    op_as_typed_op!();
    op_as_pulsed_op!();
}

impl<D: DimLike + ToDim> StatelessOp for Slice<D> {
    /// Evaluates the operation given the input tensors.
    fn eval(&self, mut inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let input = args_1!(inputs);
        Ok(tvec!(dispatch_datum!(Self::eval_t(input.datum_type())(self, input))?))
    }
}

impl<D: DimLike + ToDim> InferenceRulesOp for Slice<D> {
    fn rules<'r, 'p: 'r, 's: 'r>(
        &'s self,
        s: &mut Solver<'r>,
        inputs: &'p [TensorProxy],
        outputs: &'p [TensorProxy],
    ) -> InferenceResult {
        check_input_arity(&inputs, 1)?;
        check_output_arity(&outputs, 1)?;
        s.equals(&inputs[0].rank, &outputs[0].rank)?;
        s.equals(&inputs[0].datum_type, &outputs[0].datum_type)?;
        s.given(&inputs[0].rank, move |s, rank| {
            (0..(rank as usize)).try_for_each(move |axis| {
                if self.axis == axis {
                    s.equals(&outputs[0].shape[axis], &(self.end.clone() - &self.start).to_dim())
                } else {
                    s.equals(&outputs[0].shape[axis], &inputs[0].shape[axis])
                }
            })
        })?;
        Ok(())
    }

    inference_op_as_op!();
    to_typed!();
}

impl<D: DimLike + ToDim> TypedOp for Slice<D> {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        let mut fact = inputs[0].clone();
        fact.shape.set_dim(self.axis, (self.end.clone() - &self.start).to_dim())?;
        Ok(tvec!(fact))
    }

    fn invariants(&self, model: &TypedModel, node: &TypedNode) -> TractResult<Invariants> {
        let fact = model.outlet_fact(node.inputs[0])?;
        let axes = (0..fact.shape.rank())
            .filter(|&ax| self.axis != ax)
            .map(|axis| AxisInfo::simple(axis))
            .collect();
        Ok(axes)
    }

    fn declutter(
        &self,
        model: &TypedModel,
        node: &TypedNode,
    ) -> TractResult<Option<TypedModelPatch>> {
        let prec = model.node(node.inputs[0].node);
        if self.start == D::zero()
            && (self.end.clone().to_dim()
                == model.outlet_fact(node.inputs[0])?.shape.dim(self.axis))
        {
            return Ok(Some(TypedModelPatch::shunt_one_op(model, node)?));
        }
        let (start, end) = if let (Ok(s), Ok(e)) = (self.start.to_integer(), self.end.to_integer())
        {
            (s as usize, e as usize)
        } else {
            return Ok(None);
        };
        if let Some(concat) = prec.op_as::<super::concat::NormConcat>() {
            if concat.axis == self.axis {
                let mut offset = 0;
                for &input in &prec.inputs {
                    let len: usize = if let Ok(i) =
                        model.outlet_fact(input)?.shape.dim(self.axis).to_integer()
                    {
                        i as usize
                    } else {
                        return Ok(None);
                    };
                    if start >= offset && end <= offset + len {
                        let mut patch = TypedModelPatch::default();
                        let tap = patch.tap_model(model, input)?;
                        let slice = patch.wire_node(
                            &*node.name,
                            Slice { axis: self.axis, start: start - offset, end: end - offset },
                            &[tap],
                        )?[0];
                        patch.shunt_outside(OutletId::new(node.id, 0), slice)?;
                        return Ok(Some(patch));
                    }
                    offset += len;
                }
            }
        }
        Ok(None)
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
        let fact = target.outlet_fact(input)?.clone();
        let op: Box<dyn PulsedOp> = if self.axis == fact.axis {
            let skip = self.start.to_integer()? as usize;
            let take = (self.end.clone() - &self.start).to_dim();
            PulsedAxisSlice::new(self.axis, skip, take).into()
        } else {
            objekt::clone_box(self)
        };
        target.wire_node(&*node.name, op, &[input])
    }

    typed_op_as_op!();
}

impl<D: DimLike + ToDim> PulsedOp for Slice<D> {
    fn pulsed_output_facts(&self, inputs: &[&PulsedFact]) -> TractResult<TVec<PulsedFact>> {
        let mut fact = inputs[0].clone();
        fact.delay += self.start.to_integer()? as usize;
        fact.dim = (self.end.clone() - &self.start).to_dim();
        Ok(tvec!(fact))
    }

    pulsed_op_as_op!();
    pulsed_op_to_typed_op!();
}

#[derive(Debug, Clone, new, Default)]
pub struct PulsedAxisSlice {
    pub axis: usize,
    pub skip: usize,
    pub take: TDim,
}

impl Op for PulsedAxisSlice {
    fn name(&self) -> Cow<str> {
        "PulsedAxisSlice".into()
    }

    fn info(&self) -> TractResult<Vec<String>> {
        Ok(vec![format!("axis:{}, skip:{} take:{}", self.axis, self.skip, self.take)])
    }

    not_a_typed_op!();
    op_as_pulsed_op!();
}

impl StatelessOp for PulsedAxisSlice {
    fn eval(&self, inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        Ok(inputs)
    }
}

impl PulsedOp for PulsedAxisSlice {
    fn pulsed_output_facts(&self, inputs: &[&PulsedFact]) -> TractResult<TVec<PulsedFact>> {
        let mut fact = inputs[0].clone();
        fact.delay += self.skip;
        fact.dim = self.take.clone();
        Ok(tvec!(fact))
    }

    fn to_typed(&self) -> Box<dyn TypedOp> {
        Box::new(crate::ops::identity::Identity::default())
    }

    pulsed_op_as_op!();
}
