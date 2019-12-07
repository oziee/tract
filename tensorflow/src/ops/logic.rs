use crate::model::ParsingContext;
use crate::model::TfOpRegister;
use crate::tfpb::tensorflow::NodeDef;
use tract_core::internal::*;
use tract_core::ops as tractops;

pub fn register_all_ops(reg: &mut TfOpRegister) {
    reg.insert("Equal", |_, _| Ok(Box::new(tractops::logic::equals::bin())));
    reg.insert("Greater", |_, _| Ok(Box::new(tractops::logic::greater::bin())));
    reg.insert("GreaterEqual", |_, _| Ok(Box::new(tractops::logic::greater_equal::bin())));
    reg.insert("Less", |_, _| Ok(Box::new(tractops::logic::lesser::bin())));
    reg.insert("LessEqual", |_, _| Ok(Box::new(tractops::logic::lesser_equal::bin())));
    reg.insert("LogicalAnd", |_, _| Ok(Box::new(tractops::logic::and::bin())));
    reg.insert("LogicalOr", |_, _| Ok(Box::new(tractops::logic::or::bin())));
    reg.insert("Merge", merge);
    reg.insert("Switch", switch);
}

fn switch(ctx: &ParsingContext, pb: &NodeDef) -> TractResult<Box<dyn InferenceOp>> {
    let arity = ctx.node_output_arities[&pb.name];
    Ok(Box::new(Switch::new(arity)))
}

#[derive(Debug, Clone, new)]
pub struct Switch {
    output_arity: usize,
}

impl Op for Switch {
    fn name(&self) -> Cow<str> {
        "tf.Switch".into()
    }

    op_as_typed_op!();
}

impl StatelessOp for Switch {
    fn eval(&self, mut inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let (input, pred) = args_2!(inputs);
        let null = unsafe { Tensor::null_dt(input.datum_type(), input.shape())? };
        if *pred.to_scalar::<bool>()? {
            Ok(tvec!(null.into(), input))
        } else {
            Ok(tvec!(input, null.into()))
        }
    }
}

impl InferenceRulesOp for Switch {
    fn rules<'r, 'p: 'r, 's: 'r>(
        &'s self,
        s: &mut Solver<'r>,
        inputs: &'p [TensorProxy],
        outputs: &'p [TensorProxy],
    ) -> InferenceResult {
        check_input_arity(&inputs, 2)?;
        check_output_arity(&outputs, self.output_arity)?;
        s.equals(&inputs[1].datum_type, DatumType::Bool)?;
        s.equals(&inputs[1].shape, shapefact!())?;
        for i in 0..outputs.len() {
            s.equals(&inputs[0].datum_type, &outputs[i].datum_type)?;
            s.equals(&inputs[0].shape, &outputs[i].shape)?;
        }
        Ok(())
    }

    fn nboutputs(&self) -> TractResult<usize> {
        Ok(self.output_arity)
    }

    inference_op_as_op!();
    to_typed!();
}

impl TypedOp for Switch {
    typed_op_as_op!();

    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        let fact = TypedFact::dt_shape(f32::datum_type(), inputs[0].shape.clone())?;
        Ok(tvec!(fact.clone(), fact))
    }
}

fn merge(_ctx: &ParsingContext, pb: &NodeDef) -> TractResult<Box<dyn InferenceOp>> {
    let inputs = pb.get_attr_int::<i32>("N")?;
    Ok(Box::new(Merge::new(inputs as usize)))
}

#[derive(Debug, Clone, new)]
pub struct Merge {
    n: usize,
}

impl Op for Merge {
    fn name(&self) -> Cow<str> {
        "tf.Merge".into()
    }

    op_as_typed_op!();
}

impl StatelessOp for Merge {
    fn eval(&self, mut inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let index =
            inputs.iter().position(|t| !t.is_null()).ok_or("No tensor received in merge")?;
        Ok(tvec!(inputs.remove(index), Tensor::from(index as i32).into()))
    }
}

impl InferenceRulesOp for Merge {
    fn rules<'r, 'p: 'r, 's: 'r>(
        &'s self,
        s: &mut Solver<'r>,
        inputs: &'p [TensorProxy],
        outputs: &'p [TensorProxy],
    ) -> InferenceResult {
        check_input_arity(&inputs, self.n)?;
        check_output_arity(&outputs, 1)?;
        for i in 1..self.n {
            s.equals(&inputs[0].datum_type, &inputs[i].datum_type)?;
            s.equals(&inputs[0].shape, &inputs[i].shape)?;
        }
        s.equals(&inputs[0].datum_type, &outputs[0].datum_type)?;
        s.equals(&inputs[0].shape, &outputs[0].shape)?;
        Ok(())
    }

    inference_op_as_op!();
    to_typed!();
}

impl TypedOp for Merge {
    typed_op_as_op!();

    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(
            TypedFact::dt_shape(f32::datum_type(), inputs[0].shape.clone())?,
            TypedFact::dt_shape(i32::datum_type(), ())?
        ))
    }
}
