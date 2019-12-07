use tract_core::internal::*;

use crate::model::ParsingContext;
use crate::model::TfOpRegister;
use crate::tfpb::tensorflow::NodeDef;

pub fn register_all_ops(reg: &mut TfOpRegister) {
    reg.insert("FakeQuantWithMinMaxVars", fake_quant_with_min_max_vars);
}

fn fake_quant_with_min_max_vars(
    _ctx: &ParsingContext,
    node: &NodeDef,
) -> TractResult<Box<dyn InferenceOp>> {
    let narrow_range = node.get_attr_bool("narrow_range")?;
    let num_bits = node.get_attr_int("num_bits")?;
    Ok(Box::new(FakeQuantWithMinMaxVars::new(narrow_range, num_bits)))
}

#[derive(Clone, Debug, new)]
struct FakeQuantWithMinMaxVars {
    narrow_range: bool,
    num_bits: usize,
}

impl Op for FakeQuantWithMinMaxVars {
    fn name(&self) -> Cow<str> {
        "tf.FakeQuantWithMinMaxVars".into()
    }

    op_as_typed_op!();
}

impl StatelessOp for FakeQuantWithMinMaxVars {
    fn eval(&self, mut inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let (input, min, max) = args_3!(inputs);
        let min = min.to_scalar::<f32>()?;
        let max = max.to_scalar::<f32>()?;
        let amplitude = max - min;
        let scale_len = 2_usize.pow(self.num_bits as u32) - 1 - self.narrow_range as usize;
        let step = amplitude / scale_len as f32;
        let mut tensor = input.into_tensor().into_array::<f32>()?;
        tensor.mapv_inplace(|v| ((v - min) / step).round() * step + min);
        Ok(tvec!(tensor.into_arc_tensor()))
    }
}

impl InferenceRulesOp for FakeQuantWithMinMaxVars {
    fn rules<'r, 'p: 'r, 's: 'r>(
        &'s self,
        s: &mut Solver<'r>,
        inputs: &'p [TensorProxy],
        outputs: &'p [TensorProxy],
    ) -> InferenceResult {
        check_input_arity(&inputs, 3)?;
        check_output_arity(&outputs, 1)?;
        s.equals(&inputs[0].datum_type, &inputs[1].datum_type)?;
        s.equals(&inputs[0].datum_type, &inputs[2].datum_type)?;
        s.equals(&inputs[1].shape, shapefact!())?;
        s.equals(&inputs[2].shape, shapefact!())?;
        s.equals(&inputs[0].datum_type, &outputs[0].datum_type)?;
        s.equals(&inputs[0].shape, &outputs[0].shape)?;
        Ok(())
    }

    inference_op_as_op!();
    to_typed!();
}

impl TypedOp for FakeQuantWithMinMaxVars {
    typed_op_as_op!();

    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(TypedFact::dt_shape(f32::datum_type(), inputs[0].shape.clone())?))
    }
}
