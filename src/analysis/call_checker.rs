use crate::analysis::annotation::Annotation;
use crate::analysis::ast_visitor::{traverse, ASTVisitor, TypedVar};
use crate::analysis::{AnalysisPass, AnalysisResult};
use crate::clarity::analysis::analysis_db::AnalysisDatabase;
pub use crate::clarity::analysis::types::ContractAnalysis;
use crate::clarity::ast::ContractAST;
use crate::clarity::diagnostic::{DiagnosableError, Diagnostic, Level};
use crate::clarity::representations::SymbolicExpression;
use crate::clarity::types::{PrincipalData, QualifiedContractIdentifier, Value};
use crate::clarity::ClarityName;
use std::collections::{BTreeSet, HashMap};

pub struct CallChecker<'a> {
    diagnostics: Vec<Diagnostic>,
    // For each user-defined function, record the parameter count.
    user_funcs: HashMap<&'a ClarityName, usize>,
    // For each call of a user-defined function which has not been defined yet,
    // record the argument count, to check later.
    user_calls: Vec<(&'a ClarityName, &'a SymbolicExpression, usize)>,
}

impl<'a> CallChecker<'a> {
    fn new() -> CallChecker<'a> {
        Self {
            diagnostics: Vec::new(),
            user_funcs: HashMap::new(),
            user_calls: Vec::new(),
        }
    }

    fn run(mut self, contract_analysis: &'a ContractAnalysis) -> AnalysisResult {
        traverse(&mut self, &contract_analysis.expressions);
        self.check_user_calls();

        if self.diagnostics.len() > 0 {
            Err(self.diagnostics)
        } else {
            Ok(vec![])
        }
    }

    fn check_user_calls(&mut self) {
        for i in 0..self.user_calls.len() {
            let (name, call_expr, num_args) = self.user_calls[i];
            if let Some(&num_params) = self.user_funcs.get(name) {
                if num_args != num_params {
                    let diagnostic =
                        self.generate_diagnostic(call_expr, name, num_params, num_args);
                    self.diagnostics.push(diagnostic);
                }
            }
        }
    }

    fn generate_diagnostic(
        &mut self,
        expr: &'a SymbolicExpression,
        name: &'a ClarityName,
        expected: usize,
        got: usize,
    ) -> Diagnostic {
        Diagnostic {
            level: Level::Error,
            message: format!(
                "incorrect number of arguments in call to '{}' (expected {} got {})",
                name, expected, got
            ),
            spans: vec![expr.span.clone()],
            suggestion: None,
        }
    }
}

impl<'a> ASTVisitor<'a> for CallChecker<'a> {
    fn visit_define_private(
        &mut self,
        expr: &'a SymbolicExpression,
        name: &'a ClarityName,
        parameters: Option<Vec<TypedVar<'a>>>,
        body: &'a SymbolicExpression,
    ) -> bool {
        let num_params = match parameters {
            Some(parameters) => parameters.len(),
            None => 0,
        };
        self.user_funcs.insert(name, num_params);
        true
    }

    fn visit_define_public(
        &mut self,
        expr: &'a SymbolicExpression,
        name: &'a ClarityName,
        parameters: Option<Vec<TypedVar<'a>>>,
        body: &'a SymbolicExpression,
    ) -> bool {
        let num_params = match parameters {
            Some(parameters) => parameters.len(),
            None => 0,
        };
        self.user_funcs.insert(name, num_params);
        true
    }

    fn visit_define_read_only(
        &mut self,
        expr: &'a SymbolicExpression,
        name: &'a ClarityName,
        parameters: Option<Vec<TypedVar<'a>>>,
        body: &'a SymbolicExpression,
    ) -> bool {
        let num_params = match parameters {
            Some(parameters) => parameters.len(),
            None => 0,
        };
        self.user_funcs.insert(name, num_params);
        true
    }

    fn visit_call_user_defined(
        &mut self,
        expr: &'a SymbolicExpression,
        name: &'a ClarityName,
        args: &'a [SymbolicExpression],
    ) -> bool {
        if let Some(param_count) = self.user_funcs.get(name) {
            let param_count = *param_count;
            if args.len() != param_count {
                let diagnostic = self.generate_diagnostic(expr, name, param_count, args.len());
                self.diagnostics.push(diagnostic);
            }
        } else {
            self.user_calls.push((name, expr, args.len()));
        }
        true
    }
}

impl AnalysisPass for CallChecker<'_> {
    fn run_pass(
        contract_analysis: &mut ContractAnalysis,
        analysis_db: &mut AnalysisDatabase,
        annotations: &Vec<Annotation>,
    ) -> AnalysisResult {
        let tc = CallChecker::new();
        tc.run(contract_analysis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repl::session::Session;
    use crate::repl::SessionSettings;

    #[test]
    fn define_private() {
        let mut session = Session::new(SessionSettings::default());
        let snippet = "
(define-private (foo (amount uint))
    (ok amount)
)

(define-public (main)
    (ok (foo u1 u2))
)
"
        .to_string();
        match session.formatted_interpretation(snippet, Some("checker".to_string()), false, None) {
            Err(output) => {
                assert_eq!(output.len(), 3);
                assert_eq!(
                    output[0],
                    format!(
                        "checker:7:9: {}: incorrect number of arguments in call to 'foo' (expected 1 got 2)",
                        red!("error")
                    )
                );
                assert_eq!(output[1], "    (ok (foo u1 u2))");
                assert_eq!(output[2], "        ^~~~~~~~~~~");
            }
            _ => panic!("Expected error"),
        };
    }

    #[test]
    fn define_read_only() {
        let mut session = Session::new(SessionSettings::default());
        let snippet = "
(define-read-only (foo (amount uint))
    (ok amount)
)

(define-public (main)
    (ok (foo))
)
"
        .to_string();
        match session.formatted_interpretation(snippet, Some("checker".to_string()), false, None) {
            Err(output) => {
                assert_eq!(output.len(), 3);
                assert_eq!(
                    output[0],
                    format!(
                        "checker:7:9: {}: incorrect number of arguments in call to 'foo' (expected 1 got 0)",
                        red!("error")
                    )
                );
                assert_eq!(output[1], "    (ok (foo))");
                assert_eq!(output[2], "        ^~~~~");
            }
            _ => panic!("Expected error"),
        };
    }

    #[test]
    fn define_public() {
        let mut session = Session::new(SessionSettings::default());
        let snippet = "
(define-public (foo (amount uint))
    (ok amount)
)

(define-public (main)
    (ok (foo u1 u2))
)
"
        .to_string();
        match session.formatted_interpretation(snippet, Some("checker".to_string()), false, None) {
            Err(output) => {
                assert_eq!(output.len(), 3);
                assert_eq!(
                    output[0],
                    format!(
                        "checker:7:9: {}: incorrect number of arguments in call to 'foo' (expected 1 got 2)",
                        red!("error")
                    )
                );
                assert_eq!(output[1], "    (ok (foo u1 u2))");
                assert_eq!(output[2], "        ^~~~~~~~~~~");
            }
            _ => panic!("Expected error"),
        };
    }

    #[test]
    fn correct_call() {
        let mut session = Session::new(SessionSettings::default());
        let snippet = "
(define-private (foo (amount uint))
    (ok amount)
)

(define-public (main)
    (ok (foo u1))
)
"
        .to_string();
        match session.formatted_interpretation(snippet, Some("checker".to_string()), false, None) {
            Ok((_, result)) => {
                assert_eq!(result.diagnostics.len(), 0);
            }
            _ => panic!("Expected successful interpretation"),
        };
    }
}
