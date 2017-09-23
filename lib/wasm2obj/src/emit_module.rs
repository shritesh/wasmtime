use cretonne::Context;
use cretonne::settings;
use cretonne::isa::TargetIsa;
use cretonne::verify_function;
use cretonne::verifier;
use cretonne::settings::Configurable;
use cretonne::result::CtonError;
use cretonne::ir::entities::AnyEntity;
use cretonne::ir::{self, Ebb, FuncRef, JumpTable, Function};
use cretonne::binemit::{RelocSink, Reloc, CodeOffset};
use cton_wasm::TranslationResult;
use std::collections::HashMap;
use std::fmt::Write;
use faerie::Artifact;
use wasmstandalone::StandaloneRuntime;

type RelocRef = u16;

// Implementation of a relocation sink that just saves all the information for later
struct FaerieRelocSink {
    ebbs: HashMap<RelocRef, (Ebb, CodeOffset)>,
    funcs: HashMap<RelocRef, (FuncRef, CodeOffset)>,
    jts: HashMap<RelocRef, (JumpTable, CodeOffset)>,
}

impl RelocSink for FaerieRelocSink {
    fn reloc_ebb(&mut self, offset: CodeOffset, reloc: Reloc, ebb: Ebb) {
        self.ebbs.insert(reloc.0, (ebb, offset));
    }
    fn reloc_func(&mut self, offset: CodeOffset, reloc: Reloc, func: FuncRef) {
        self.funcs.insert(reloc.0, (func, offset));
    }
    fn reloc_jt(&mut self, offset: CodeOffset, reloc: Reloc, jt: JumpTable) {
        self.jts.insert(reloc.0, (jt, offset));
    }
}

impl FaerieRelocSink {
    fn new() -> FaerieRelocSink {
        FaerieRelocSink {
            ebbs: HashMap::new(),
            funcs: HashMap::new(),
            jts: HashMap::new(),
        }
    }
}

/// Emits a module that has been emitted with the `WasmRuntime` runtime
/// implementation to a native object file.
pub fn emit_module(
    trans_result: &TranslationResult,
    obj: &mut Artifact,
    isa: &TargetIsa,
    runtime: &StandaloneRuntime,
) -> Result<(), String> {
    debug_assert!(
        trans_result.start_index.is_none() ||
            trans_result.start_index.unwrap() >= runtime.imported_funcs.len(),
        "imported start functions not supported yet"
    );

    let mut shared_builder = settings::builder();
    shared_builder.enable("enable_verifier").expect(
        "Missing enable_verifier setting",
    );

    for function in &trans_result.functions {
        let mut context = Context::new();
        verify_function(function, isa).unwrap();
        context.func = function.clone(); // TODO: Avoid this clone.
        let code_size = context.compile(&*isa).map_err(|e| {
            pretty_error(&context.func, Some(isa), e)
        })? as usize;
        if code_size == 0 {
            return Err(String::from("no code generated by Cretonne"));
        }
        let mut code_buf: Vec<u8> = Vec::with_capacity(code_size);
        code_buf.resize(code_size, 0);
        let mut relocsink = FaerieRelocSink::new();
        context.emit_to_memory(code_buf.as_mut_ptr(), &mut relocsink, &*isa);

        // FIXME: get the real linkage name of the function
        obj.add_code("the_function_name", code_buf);

        assert!(relocsink.jts.is_empty(), "jump tables not yet implemented");
        assert!(relocsink.ebbs.is_empty(), "ebb relocs not yet implemented");
        assert!(
            relocsink.funcs.is_empty(),
            "function relocs not yet implemented"
        );

        // FIXME: handle imports
    }

    Ok(())
}

/// Pretty-print a verifier error.
fn pretty_verifier_error(func: &Function, isa: Option<&TargetIsa>, err: verifier::Error) -> String {
    let mut msg = err.to_string();
    match err.location {
        AnyEntity::Inst(inst) => {
            write!(msg, "\n{}: {}\n\n", inst, func.dfg.display_inst(inst, isa)).unwrap()
        }
        _ => msg.push('\n'),
    }
    write!(msg, "{}", func.display(isa)).unwrap();
    msg
}

/// Pretty-print a Cretonne error.
fn pretty_error(func: &ir::Function, isa: Option<&TargetIsa>, err: CtonError) -> String {
    if let CtonError::Verifier(e) = err {
        pretty_verifier_error(func, isa, e)
    } else {
        err.to_string()
    }
}