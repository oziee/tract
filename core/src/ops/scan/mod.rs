use crate::internal::*;
use std::fmt;

mod codegen;
mod typed;

pub use typed::TypedScan;

#[derive(Clone, new, Hash)]
pub enum InputMapping<C: Clone> {
    Full { slot: usize },
    State { initializer: StateInitializer },
    Scan { slot: usize, axis: usize, chunk: C },
}

impl<C: Clone> InputMapping<C> {
    pub fn as_state(&self) -> Option<&StateInitializer> {
        match self {
            InputMapping::State { initializer } => Some(initializer),
            _ => None,
        }
    }

    pub fn as_scan(&self) -> Option<(usize, usize, C)> {
        match self {
            InputMapping::Scan { slot, axis, chunk } => Some((*slot, *axis, chunk.clone())),
            _ => None,
        }
    }

    pub fn invisible(&self) -> bool {
        if let InputMapping::State { initializer: StateInitializer::Value(_) } = self {
            true
        } else {
            false
        }
    }

    pub fn slot(&self) -> Option<usize> {
        match self {
            InputMapping::Full { slot } => Some(*slot),
            InputMapping::Scan { slot, .. } => Some(*slot),
            InputMapping::State { initializer } => match initializer {
                StateInitializer::FromInput(slot) => Some(*slot),
                _ => None,
            },
        }
    }
}

impl<C: Clone + fmt::Debug> fmt::Debug for InputMapping<C> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            InputMapping::Full { slot } => write!(fmt, "Full, inlet {}", slot),
            InputMapping::State { initializer } => {
                write!(fmt, "State initialized by {:?}", initializer)
            }
            InputMapping::Scan { slot, axis, chunk } => {
                write!(fmt, "Scan inlet {}, axis: {}, chunk: {:?}.", slot, axis, chunk)
            }
        }
    }
}

#[derive(Clone, new, Hash)]
pub struct OutputMapping<C: Clone, F: Clone> {
    pub full_slot: Option<usize>,
    pub axis: usize,
    pub chunk: C,
    pub full_dim_hint: Option<F>,
    pub last_value_slot: Option<usize>,
    pub state: bool,
}

impl<C: Clone, F: Clone> OutputMapping<C, F> {
    pub fn invisible(&self) -> bool {
        self.full_slot.is_none() && self.last_value_slot.is_none()
    }
}

impl<C: Clone, F: Clone> fmt::Debug for OutputMapping<C, F> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if self.state {
            write!(fmt, "State. ")?;
        }
        if let Some(last_value_slot) = self.last_value_slot {
            write!(fmt, "Last value to outlet {}. ", last_value_slot)?;
        }
        if let Some(full_slot) = self.full_slot {
            write!(fmt, "Full value to outlet {}. ", full_slot)?;
        }
        write!(fmt, "Axis:{} ", self.axis)?;
        Ok(())
    }
}

#[derive(Clone, new, Hash)]
pub enum StateInitializer {
    FromInput(usize),
    Value(Arc<Tensor>),
}

impl fmt::Debug for StateInitializer {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use StateInitializer::*;
        match self {
            FromInput(i) => write!(fmt, "inlet {}", i),
            Value(t) => write!(fmt, "tensor {:?}", t),
        }
    }
}
