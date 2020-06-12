use crate::annotations::*;
use crate::display_params::*;
use crate::errors::*;
use crate::terminal;
use crate::{BenchLimits, Parameters};
use tract_hir::internal::*;

pub fn handle(
    params: &Parameters,
    options: &DisplayParams,
    matches: &clap::ArgMatches,
    bench_limits: &BenchLimits,
    _inner: Vec<String>,
) -> CliResult<()> {
    let model = &*params.tract_model;
    let mut annotations = Annotations::from_model(model)?.with_graph_def(model, &params.graph)?;
    if options.cost {
        annotations.extract_costs(model)?;
    }
    if options.profile {
        let model = params
            .tract_model
            .downcast_ref::<TypedModel>()
            .ok_or("Can only profile typed models")?;
        crate::profile::profile(model, bench_limits, &mut annotations)?;
    }

    if let Some(asserts) = &params.assertions {
        if let Some(asserts) = &asserts.assert_output_facts {
            let outputs_facts: Vec<InferenceFact> = model
                .output_outlets()
                .iter()
                .map(|o| Ok(InferenceFact::from(&model.outlet_typedfact(*o)?)))
                .collect::<TractResult<Vec<InferenceFact>>>()?;
            crate::utils::check_inferred(&*outputs_facts, &*asserts)?;
        }
    }

    if options.cost {
        let total = annotations.tags.values().sum::<NodeTags>();
        let assert = matches.value_of("assert-cost").map(|a| crate::cost::parse_costs(a));
        if let Some(assert) = assert {
            let assert: HashMap<Cost, TDim> =
                assert.iter().map(|(c, n)| (*c, n.to_dim())).collect();
            let total = total.cost.iter().cloned().collect::<HashMap<_, _>>();
            if assert != total {
                bail!("Cost assertion not met: expected {:?} got {:?}", assert, total);
            }
        }
    }

    if !options.quiet {
        if options.json {
            let export = crate::export::GraphPerfInfo::from(model, &annotations);
            serde_json::to_writer(std::io::stdout(), &export)?;
        } else {
            terminal::render(model, &annotations, options)?;
            terminal::render_summaries(model, &annotations, options)?;
        }
    }

    Ok(())
}
