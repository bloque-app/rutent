//! The calls a route makes (router-api 3.0.0, `T-011-06`, `REQ-MDL-21`, `REQ-MDL-9`, `THR-86`).
//!
//! The platform lets a module call another provider's module only where its release declares the call and a
//! segment of its route names it. Each segment here names the calls its provider's module declares, in the
//! snapshot's calls figure of the release its tuple comes from (same provider and `module_version`), whose callee
//! is the provider of another segment of the same part: every kind declared for that callee, in the order the
//! figure lists them. A declared call to a provider on no segment of the part stays unnamed, so a module can never
//! reach a provider its route doesn't take, and a module whose release the snapshot doesn't state names nothing.
//! A segment that names a call cites the calls figure; one that names none cites it not.

use rutent_contracts::router_api::{
    CatalogueSnapshot, Figure, FigureValue, Id, NonEmpty, Operation, Origin, RouteCall,
};

/// A provider figure's module release: its provider and `module_version`.
fn release(figure: &Figure) -> Option<(&Id, &str)> {
    match &figure.origin {
        Origin::Provider {
            provider,
            module_version,
            ..
        } => Some((provider, module_version.as_str())),
        Origin::Operator { .. } => None,
    }
}

/// Names, on each operation of one part, the calls it makes, and cites the calls figure they come from.
pub(crate) fn name(snapshot: &CatalogueSnapshot, operations: &mut [Operation]) {
    let figure = |id: &Id| snapshot.figures.iter().find(|f| &f.figure_id == id);
    let providers: Vec<Id> = operations.iter().map(|o| o.provider.clone()).collect();
    for (i, operation) in operations.iter_mut().enumerate() {
        // The tuple is the first figure an operation cites.
        let Some(tuple) = figure(operation.figures.first()) else {
            continue;
        };
        let Some(declared) = snapshot
            .figures
            .iter()
            .find(|f| matches!(f.value, FigureValue::Calls { .. }) && release(f) == release(tuple))
        else {
            continue;
        };
        let FigureValue::Calls { calls } = &declared.value else {
            continue;
        };
        let on_the_part = |callee: &Id| {
            callee != &operation.provider
                && providers
                    .iter()
                    .enumerate()
                    .any(|(j, p)| j != i && p == callee)
        };
        let mut named: Vec<RouteCall> = Vec::new();
        for call in calls.iter().filter(|c| on_the_part(&c.callee)) {
            for &kind in call.kinds.iter() {
                let route_call = RouteCall {
                    callee: call.callee.clone(),
                    kind,
                };
                if !named.contains(&route_call) {
                    named.push(route_call);
                }
            }
        }
        if named.is_empty() {
            continue;
        }
        let mut figures: Vec<Id> = operation.figures.clone().into();
        figures.push(declared.figure_id.clone());
        operation.figures = NonEmpty::new(figures).expect("the tuple and the calls figure");
        operation.calls = named;
    }
}
