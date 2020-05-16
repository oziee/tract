use crate::internal::*;

#[derive(Debug, Clone, new, Hash)]
pub struct Dummy;

impl Op for Dummy {
    fn name(&self) -> Cow<str> {
        "Dummy".into()
    }

    op_as_typed_op!();
    not_a_pulsed_op!();
}

tract_linalg::impl_dyn_hash!(Dummy);

impl StatelessOp for Dummy {
    fn eval(&self, _inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        bail!("eval() called on a Dummy op. This is a bug.")
    }
}

impl TypedOp for Dummy {
    as_op!();

    fn output_facts(&self, _inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!())
    }
}
