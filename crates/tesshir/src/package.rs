use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::parser::parse_program;
use crate::span::{Node, SourceFile, Span};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path as FsPath, PathBuf};

const STDLIB_OPTION: &str = include_str!("../stdlib/std/option.tess");

#[derive(Clone, Debug)]
pub struct PackageOptions {
    pub include_stdlib: bool,
}

impl Default for PackageOptions {
    fn default() -> Self {
        Self {
            include_stdlib: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Package {
    pub program: Program,
    pub source_map: PackageSourceMap,
    pub modules: Vec<PackageModule>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageModule {
    pub name: Vec<String>,
    pub source_name: String,
}

#[derive(Clone, Debug, Default)]
pub struct PackageSourceMap {
    sources: Vec<PackageSource>,
}

#[derive(Clone, Debug)]
struct PackageSource {
    name: String,
    text: String,
    base: usize,
    len: usize,
}

#[derive(Clone, Debug)]
pub struct PackageLoadError {
    pub diagnostics: Vec<Diagnostic>,
    pub source_map: PackageSourceMap,
}

impl PackageSourceMap {
    pub fn render_diagnostic(&self, diagnostic: &Diagnostic) -> String {
        let Some(source) = self.source_for_span(diagnostic.span) else {
            return format!("{:?}: {}", diagnostic.severity, diagnostic.message);
        };
        let local_span = Span::new(
            diagnostic.span.start.saturating_sub(source.base),
            diagnostic.span.end.saturating_sub(source.base),
        );
        let source_file = SourceFile::new(&source.name, &source.text);
        format!(
            "{:?}: {}: {}",
            diagnostic.severity,
            source_file.format_span(local_span),
            diagnostic.message
        )
    }

    fn add_source(&mut self, name: String, text: String, base: usize) {
        let len = text.len();
        self.sources.push(PackageSource {
            name,
            text,
            base,
            len,
        });
    }

    fn source_for_span(&self, span: Span) -> Option<&PackageSource> {
        self.sources.iter().find(|source| {
            span.start >= source.base && span.start <= source.base.saturating_add(source.len)
        })
    }
}

pub fn load_package(
    input: impl AsRef<FsPath>,
    options: &PackageOptions,
) -> Result<Package, PackageLoadError> {
    let input = input.as_ref();
    let mut sources = vec![];

    if options.include_stdlib {
        sources.push(SourceSpec::stdlib(
            "std/option.tess",
            &["std", "option"],
            STDLIB_OPTION,
        ));
    }

    let user_sources = match discover_user_sources(input) {
        Ok(sources) => sources,
        Err(err) => {
            return Err(PackageLoadError {
                diagnostics: vec![Diagnostic::error(Span::default(), err.to_string())],
                source_map: PackageSourceMap::default(),
            });
        }
    };
    sources.extend(user_sources);

    if sources.iter().all(|source| source.is_stdlib) {
        return Err(PackageLoadError {
            diagnostics: vec![Diagnostic::error(
                Span::default(),
                format!("no `.tess` source files found under `{}`", input.display()),
            )],
            source_map: PackageSourceMap::default(),
        });
    }

    let mut source_map = PackageSourceMap::default();
    let mut modules = vec![];
    let mut items = vec![];
    let mut diagnostics = vec![];
    let mut base = 0usize;

    for source in sources {
        let text = match source.read_text() {
            Ok(text) => text,
            Err(err) => {
                diagnostics.push(Diagnostic::error(
                    Span::new(base, base),
                    format!("failed to read `{}`: {err}", source.name),
                ));
                continue;
            }
        };
        let source_name = source.name.clone();
        source_map.add_source(source_name.clone(), text.clone(), base);
        modules.push(PackageModule {
            name: source.module_path.clone(),
            source_name,
        });

        match parse_program(&text) {
            Ok(mut program) => {
                let mut package_diagnostics = qualify_program(&mut program, &source.module_path);
                shift_diagnostics(&mut package_diagnostics, base);
                diagnostics.extend(package_diagnostics);
                shift_program(&mut program, base);
                items.extend(program.kind.items);
            }
            Err(mut err) => {
                shift_diagnostics(&mut err.diagnostics, base);
                diagnostics.extend(err.diagnostics);
            }
        }

        base = base.saturating_add(text.len()).saturating_add(1);
    }

    if !diagnostics.is_empty() {
        return Err(PackageLoadError {
            diagnostics,
            source_map,
        });
    }

    let span = match (items.first(), items.last()) {
        (Some(first), Some(last)) => Span::new(first.span.start, last.span.end),
        _ => Span::default(),
    };
    Ok(Package {
        program: Node::new(span, ProgramKind { items }),
        source_map,
        modules,
    })
}

#[derive(Clone, Debug)]
struct SourceSpec {
    name: String,
    path: Option<PathBuf>,
    module_path: Vec<String>,
    embedded_text: Option<&'static str>,
    is_stdlib: bool,
}

impl SourceSpec {
    fn file(name: String, path: PathBuf, module_path: Vec<String>) -> Self {
        Self {
            name,
            path: Some(path),
            module_path,
            embedded_text: None,
            is_stdlib: false,
        }
    }

    fn stdlib(name: &str, module_path: &[&str], text: &'static str) -> Self {
        Self {
            name: name.to_owned(),
            path: None,
            module_path: module_path
                .iter()
                .map(|segment| (*segment).to_owned())
                .collect(),
            embedded_text: Some(text),
            is_stdlib: true,
        }
    }

    fn read_text(&self) -> io::Result<String> {
        if let Some(text) = self.embedded_text {
            Ok(text.to_owned())
        } else {
            fs::read_to_string(self.path.as_ref().expect("file source has path"))
        }
    }
}

fn discover_user_sources(input: &FsPath) -> io::Result<Vec<SourceSpec>> {
    if input.is_file() {
        return Ok(vec![SourceSpec::file(
            input.display().to_string(),
            input.to_owned(),
            vec![],
        )]);
    }
    if !input.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("input `{}` is not a file or directory", input.display()),
        ));
    }

    let root = package_source_root(input);
    let mut paths = vec![];
    collect_tess_files(&root, &mut paths)?;
    paths.sort();

    Ok(paths
        .into_iter()
        .map(|path| {
            let module_path = module_path_for_file(&root, &path);
            SourceSpec::file(path.display().to_string(), path, module_path)
        })
        .collect())
}

fn package_source_root(input: &FsPath) -> PathBuf {
    let src = input.join("src");
    if src.is_dir() && contains_tess_file(&src) {
        src
    } else {
        input.to_owned()
    }
}

fn contains_tess_file(root: &FsPath) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if contains_tess_file(&path) {
                return true;
            }
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("tess") {
            return true;
        }
    }
    false
}

fn collect_tess_files(root: &FsPath, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if path.is_dir() {
            if file_name.starts_with('.') || file_name == "target" {
                continue;
            }
            collect_tess_files(&path, paths)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("tess") {
            paths.push(path);
        }
    }
    Ok(())
}

fn module_path_for_file(root: &FsPath, path: &FsPath) -> Vec<String> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut components = relative
        .iter()
        .map(|component| component.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if let Some(last) = components.last_mut() {
        if let Some(stripped) = last.strip_suffix(".tess") {
            *last = stripped.to_owned();
        }
    }
    if matches!(components.last().map(String::as_str), Some("main" | "lib")) {
        components.pop();
    } else if matches!(components.last().map(String::as_str), Some("mod")) {
        components.pop();
    }
    components
}

fn qualify_program(program: &mut Program, module_path: &[String]) -> Vec<Diagnostic> {
    let locals = LocalNames::new(&program.kind.items);
    let mut diagnostics = vec![];
    for item in &mut program.kind.items {
        let mut scope = ScopeNames::default();
        qualify_item_refs(item, module_path, &locals, &mut scope, &mut diagnostics);
    }
    for item in &mut program.kind.items {
        qualify_item_name(item, module_path);
    }
    diagnostics
}

#[derive(Clone, Debug, Default)]
struct LocalNames {
    types: HashSet<String>,
    interfaces: HashSet<String>,
    consts: HashSet<String>,
    fns: HashSet<String>,
    imports: HashMap<String, ImportBinding>,
    import_roots: HashSet<String>,
}

#[derive(Clone, Debug)]
struct ImportBinding {
    path: Vec<String>,
}

impl LocalNames {
    fn new(items: &[Item]) -> Self {
        let mut locals = Self::default();
        for item in items {
            match &item.kind {
                ItemKind::Use(item) => {
                    if let Some(first) = item.kind.path.kind.segments.first() {
                        locals.import_roots.insert(first.clone());
                    }
                    if let Some(alias) = item.kind.path.kind.segments.last() {
                        locals.imports.insert(
                            alias.clone(),
                            ImportBinding {
                                path: item.kind.path.kind.segments.clone(),
                            },
                        );
                    }
                }
                ItemKind::Const(item) => {
                    locals.consts.insert(item.kind.name.clone());
                }
                ItemKind::Struct(item) => {
                    locals.types.insert(item.kind.name.clone());
                }
                ItemKind::Enum(item) => {
                    locals.types.insert(item.kind.name.clone());
                }
                ItemKind::Interface(item) => {
                    locals.interfaces.insert(item.kind.name.clone());
                }
                ItemKind::Impl(_) => {}
                ItemKind::Fn(item) => {
                    locals.fns.insert(item.kind.name.clone());
                }
            }
        }
        locals
    }
}

#[derive(Clone, Debug, Default)]
struct ScopeNames {
    type_params: HashSet<String>,
    const_params: HashSet<String>,
}

fn qualify_item_name(item: &mut Item, module_path: &[String]) {
    match &mut item.kind {
        ItemKind::Use(_) => {}
        ItemKind::Const(item) => item.kind.name = qualify_name(module_path, &item.kind.name),
        ItemKind::Struct(item) => item.kind.name = qualify_name(module_path, &item.kind.name),
        ItemKind::Enum(item) => item.kind.name = qualify_name(module_path, &item.kind.name),
        ItemKind::Interface(item) => item.kind.name = qualify_name(module_path, &item.kind.name),
        ItemKind::Impl(_) => {}
        ItemKind::Fn(item) => item.kind.name = qualify_name(module_path, &item.kind.name),
    }
}

fn qualify_item_refs(
    item: &mut Item,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut item.kind {
        ItemKind::Use(_) => {}
        ItemKind::Const(item) => {
            qualify_const_expr(&mut item.kind.expr, module_path, locals, scope, diagnostics)
        }
        ItemKind::Struct(item) => {
            let generics = item.kind.generics.clone();
            with_generic_scope(scope, &generics, |scope| {
                qualify_generic_params(
                    &mut item.kind.generics,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                qualify_where_predicates(
                    &mut item.kind.where_predicates,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                for field in &mut item.kind.fields {
                    qualify_ty(&mut field.kind.ty, module_path, locals, scope, diagnostics);
                }
            });
        }
        ItemKind::Enum(item) => {
            let generics = item.kind.generics.clone();
            with_generic_scope(scope, &generics, |scope| {
                qualify_generic_params(
                    &mut item.kind.generics,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                qualify_where_predicates(
                    &mut item.kind.where_predicates,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                for variant in &mut item.kind.variants {
                    qualify_variant_payload(
                        &mut variant.kind.payload,
                        module_path,
                        locals,
                        scope,
                        diagnostics,
                    );
                    if let Some(discriminant) = &mut variant.kind.discriminant {
                        qualify_const_expr(discriminant, module_path, locals, scope, diagnostics);
                    }
                }
            });
        }
        ItemKind::Interface(item) => {
            let generics = item.kind.generics.clone();
            with_generic_scope(scope, &generics, |scope| {
                qualify_generic_params(
                    &mut item.kind.generics,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                for interface in &mut item.kind.super_interfaces {
                    qualify_interface_ref(interface, module_path, locals, scope, diagnostics);
                }
                for member in &mut item.kind.members {
                    match &mut member.kind {
                        InterfaceMemberKind::AssocConst(sig) => {
                            if let Some(default) = &mut sig.kind.default {
                                qualify_const_expr(
                                    default,
                                    module_path,
                                    locals,
                                    scope,
                                    diagnostics,
                                );
                            }
                        }
                        InterfaceMemberKind::Method(sig) => {
                            qualify_method_sig(sig, module_path, locals, scope, diagnostics);
                        }
                    }
                }
            });
        }
        ItemKind::Impl(item) => {
            let generics = item.kind.generics.clone();
            with_generic_scope(scope, &generics, |scope| {
                qualify_generic_params(
                    &mut item.kind.generics,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                qualify_where_predicates(
                    &mut item.kind.where_predicates,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                if let Some(interface) = &mut item.kind.interface {
                    qualify_interface_ref(interface, module_path, locals, scope, diagnostics);
                }
                qualify_ty(
                    &mut item.kind.self_ty,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                for member in &mut item.kind.members {
                    match &mut member.kind {
                        ImplMemberKind::AssocConst(item) => {
                            qualify_const_expr(
                                &mut item.kind.expr,
                                module_path,
                                locals,
                                scope,
                                diagnostics,
                            );
                        }
                        ImplMemberKind::Method(method) => {
                            qualify_method_sig(
                                &mut method.kind.sig,
                                module_path,
                                locals,
                                scope,
                                diagnostics,
                            );
                            qualify_expr(
                                &mut method.kind.body,
                                module_path,
                                locals,
                                scope,
                                diagnostics,
                            );
                        }
                    }
                }
            });
        }
        ItemKind::Fn(item) => {
            let generics = item.kind.generics.clone();
            with_generic_scope(scope, &generics, |scope| {
                qualify_generic_params(
                    &mut item.kind.generics,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                qualify_where_predicates(
                    &mut item.kind.where_predicates,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
                for param in &mut item.kind.params {
                    qualify_ty(&mut param.kind.ty, module_path, locals, scope, diagnostics);
                }
                qualify_ty(&mut item.kind.ret, module_path, locals, scope, diagnostics);
                qualify_expr(&mut item.kind.body, module_path, locals, scope, diagnostics);
            });
        }
    }
}

fn with_generic_scope(
    scope: &mut ScopeNames,
    generics: &[GenericParam],
    f: impl FnOnce(&mut ScopeNames),
) {
    let old_type_params = scope.type_params.clone();
    let old_const_params = scope.const_params.clone();
    for generic in generics {
        match &generic.kind {
            GenericParamKind::Type { name, .. } => {
                scope.type_params.insert(name.clone());
            }
            GenericParamKind::Const { name, .. } => {
                scope.const_params.insert(name.clone());
            }
        }
    }
    f(scope);
    scope.type_params = old_type_params;
    scope.const_params = old_const_params;
}

fn qualify_generic_params(
    generics: &mut [GenericParam],
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for generic in generics {
        if let GenericParamKind::Type { bounds, .. } = &mut generic.kind {
            for bound in bounds {
                qualify_interface_ref(bound, module_path, locals, scope, diagnostics);
            }
        }
    }
}

fn qualify_where_predicates(
    predicates: &mut [WherePredicate],
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for predicate in predicates {
        match &mut predicate.kind {
            WherePredicateKind::Implements { ty, interface } => {
                qualify_ty(ty, module_path, locals, scope, diagnostics);
                qualify_interface_ref(interface, module_path, locals, scope, diagnostics);
            }
            WherePredicateKind::ConstEq { lhs, rhs } => {
                qualify_const_expr(lhs, module_path, locals, scope, diagnostics);
                qualify_const_expr(rhs, module_path, locals, scope, diagnostics);
            }
        }
    }
}

fn qualify_method_sig(
    sig: &mut MethodSig,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let generics = sig.kind.generics.clone();
    with_generic_scope(scope, &generics, |scope| {
        qualify_generic_params(
            &mut sig.kind.generics,
            module_path,
            locals,
            scope,
            diagnostics,
        );
        for param in &mut sig.kind.params {
            qualify_ty(&mut param.kind.ty, module_path, locals, scope, diagnostics);
        }
        qualify_ty(&mut sig.kind.ret, module_path, locals, scope, diagnostics);
    });
}

fn qualify_variant_payload(
    payload: &mut VariantPayload,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut payload.kind {
        VariantPayloadKind::Unit => {}
        VariantPayloadKind::Tuple(tys) => {
            for ty in tys {
                qualify_ty(ty, module_path, locals, scope, diagnostics);
            }
        }
        VariantPayloadKind::Struct(fields) => {
            for field in fields {
                qualify_ty(&mut field.kind.ty, module_path, locals, scope, diagnostics);
            }
        }
    }
}

fn qualify_ty(
    ty: &mut Ty,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut ty.kind {
        TyKind::Unit | TyKind::Bool | TyKind::Never | TyKind::Int(_) | TyKind::SelfTy => {}
        TyKind::Path { path, args } => {
            qualify_type_path(path, module_path, locals, scope, diagnostics);
            for arg in args {
                qualify_generic_arg(arg, module_path, locals, scope, diagnostics);
            }
        }
        TyKind::Array { elem, len } => {
            qualify_ty(elem, module_path, locals, scope, diagnostics);
            qualify_const_expr(len, module_path, locals, scope, diagnostics);
        }
        TyKind::Ref { ty, .. } => qualify_ty(ty, module_path, locals, scope, diagnostics),
        TyKind::Fn { params, ret } => {
            for param in params {
                qualify_ty(param, module_path, locals, scope, diagnostics);
            }
            qualify_ty(ret, module_path, locals, scope, diagnostics);
        }
    }
}

fn qualify_generic_arg(
    arg: &mut GenericArg,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut arg.kind {
        GenericArgKind::Ty(ty) => qualify_ty(ty, module_path, locals, scope, diagnostics),
        GenericArgKind::Const(expr) => {
            qualify_const_expr(expr, module_path, locals, scope, diagnostics)
        }
    }
}

fn qualify_const_expr(
    expr: &mut ConstExpr,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut expr.kind {
        ConstExprKind::IntLit(_) | ConstExprKind::Param(_) => {}
        ConstExprKind::Path(path) => {
            qualify_const_path(path, module_path, locals, scope, diagnostics)
        }
        ConstExprKind::AssocConst {
            ty,
            interface,
            name: _,
        } => {
            qualify_ty(ty, module_path, locals, scope, diagnostics);
            qualify_interface_path(interface, module_path, locals, scope, diagnostics);
        }
        ConstExprKind::Unary { expr, .. } => {
            qualify_const_expr(expr, module_path, locals, scope, diagnostics)
        }
        ConstExprKind::Binary { lhs, rhs, .. } => {
            qualify_const_expr(lhs, module_path, locals, scope, diagnostics);
            qualify_const_expr(rhs, module_path, locals, scope, diagnostics);
        }
        ConstExprKind::Cast { expr, .. } => {
            qualify_const_expr(expr, module_path, locals, scope, diagnostics)
        }
    }
}

fn qualify_interface_ref(
    interface: &mut InterfaceRef,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    qualify_interface_path(
        &mut interface.kind.path,
        module_path,
        locals,
        scope,
        diagnostics,
    );
    for arg in &mut interface.kind.args {
        qualify_generic_arg(arg, module_path, locals, scope, diagnostics);
    }
}

fn qualify_expr(
    expr: &mut Expr,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut expr.kind {
        ExprKind::UnitLit | ExprKind::BoolLit(_) | ExprKind::IntLit(_) | ExprKind::Todo => {}
        ExprKind::Var(_) => {}
        ExprKind::Unary { expr, .. } => qualify_expr(expr, module_path, locals, scope, diagnostics),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Assign { lhs, rhs } => {
            qualify_expr(lhs, module_path, locals, scope, diagnostics);
            qualify_expr(rhs, module_path, locals, scope, diagnostics);
        }
        ExprKind::Call { callee, args } => {
            if let ExprKind::Var(name) = &mut callee.kind {
                if locals.fns.contains(name) {
                    *name = qualify_name(module_path, name);
                } else if let Some(import) = locals.imports.get(name) {
                    *name = import.path.join("::");
                }
            } else {
                qualify_expr(callee, module_path, locals, scope, diagnostics);
            }
            for arg in args {
                qualify_expr(arg, module_path, locals, scope, diagnostics);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            qualify_expr(receiver, module_path, locals, scope, diagnostics);
            for arg in args {
                qualify_expr(arg, module_path, locals, scope, diagnostics);
            }
        }
        ExprKind::StructLit { path, fields } => {
            qualify_type_path(path, module_path, locals, scope, diagnostics);
            for field in fields {
                qualify_expr(
                    &mut field.kind.expr,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
            }
        }
        ExprKind::EnumCtor {
            enum_path, args, ..
        } => {
            qualify_ctor_path(enum_path, module_path, locals, scope, diagnostics);
            qualify_enum_ctor_args(args, module_path, locals, scope, diagnostics);
        }
        ExprKind::Field { base, .. } => qualify_expr(base, module_path, locals, scope, diagnostics),
        ExprKind::Index { base, index } => {
            qualify_expr(base, module_path, locals, scope, diagnostics);
            qualify_expr(index, module_path, locals, scope, diagnostics);
        }
        ExprKind::Match { scrutinee, arms } => {
            qualify_expr(scrutinee, module_path, locals, scope, diagnostics);
            for arm in arms {
                qualify_pat(&mut arm.kind.pat, module_path, locals, scope, diagnostics);
                qualify_expr(&mut arm.kind.body, module_path, locals, scope, diagnostics);
            }
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            qualify_expr(cond, module_path, locals, scope, diagnostics);
            qualify_block(then_branch, module_path, locals, scope, diagnostics);
            if let Some(block) = else_branch {
                qualify_block(block, module_path, locals, scope, diagnostics);
            }
        }
        ExprKind::Block(block) => qualify_block(block, module_path, locals, scope, diagnostics),
        ExprKind::Return(value) => {
            if let Some(value) = value {
                qualify_expr(value, module_path, locals, scope, diagnostics);
            }
        }
    }
}

fn qualify_block(
    block: &mut Block,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for stmt in &mut block.kind.stmts {
        match &mut stmt.kind {
            StmtKind::Let(let_stmt) => {
                if let Some(ty) = &mut let_stmt.kind.ty {
                    qualify_ty(ty, module_path, locals, scope, diagnostics);
                }
                if let Some(init) = &mut let_stmt.kind.init {
                    qualify_expr(init, module_path, locals, scope, diagnostics);
                }
            }
            StmtKind::Expr(expr) | StmtKind::Semi(expr) => {
                qualify_expr(expr, module_path, locals, scope, diagnostics);
            }
        }
    }
    if let Some(tail) = &mut block.kind.tail {
        qualify_expr(tail, module_path, locals, scope, diagnostics);
    }
}

fn qualify_enum_ctor_args(
    args: &mut EnumCtorArgs,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut args.kind {
        EnumCtorArgsKind::Unit => {}
        EnumCtorArgsKind::Tuple(args) => {
            for arg in args {
                qualify_expr(arg, module_path, locals, scope, diagnostics);
            }
        }
        EnumCtorArgsKind::Struct(fields) => {
            for field in fields {
                qualify_expr(
                    &mut field.kind.expr,
                    module_path,
                    locals,
                    scope,
                    diagnostics,
                );
            }
        }
    }
}

fn qualify_pat(
    pat: &mut Pat,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut pat.kind {
        PatKind::Wildcard
        | PatKind::Binding { .. }
        | PatKind::Unit
        | PatKind::BoolLit(_)
        | PatKind::IntLit(_) => {}
        PatKind::EnumVariant {
            enum_path, args, ..
        } => {
            qualify_ctor_path(enum_path, module_path, locals, scope, diagnostics);
            qualify_enum_pat_args(args, module_path, locals, scope, diagnostics);
        }
        PatKind::Struct { path, fields } => {
            qualify_type_path(path, module_path, locals, scope, diagnostics);
            for field in fields {
                qualify_pat(&mut field.kind.pat, module_path, locals, scope, diagnostics);
            }
        }
    }
}

fn qualify_enum_pat_args(
    args: &mut EnumPatArgs,
    module_path: &[String],
    locals: &LocalNames,
    scope: &mut ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut args.kind {
        EnumPatArgsKind::Unit => {}
        EnumPatArgsKind::Tuple(pats) => {
            for pat in pats {
                qualify_pat(pat, module_path, locals, scope, diagnostics);
            }
        }
        EnumPatArgsKind::Struct(fields) => {
            for field in fields {
                qualify_pat(&mut field.kind.pat, module_path, locals, scope, diagnostics);
            }
        }
    }
}

fn qualify_type_path(
    path: &mut Path,
    module_path: &[String],
    locals: &LocalNames,
    scope: &ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if path.kind.segments.len() != 1 {
        require_explicit_dependency(path, module_path, locals, diagnostics);
        return;
    }
    let name = &path.kind.segments[0];
    if scope.type_params.contains(name) {
        return;
    }
    if locals.types.contains(name) || locals.interfaces.contains(name) {
        path.kind.segments = qualified_segments(module_path, name);
    } else if let Some(import) = locals.imports.get(name) {
        path.kind.segments = import.path.clone();
    }
}

fn qualify_interface_path(
    path: &mut Path,
    module_path: &[String],
    locals: &LocalNames,
    _scope: &ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if path.kind.segments.len() != 1 {
        require_explicit_dependency(path, module_path, locals, diagnostics);
        return;
    }
    let name = &path.kind.segments[0];
    if locals.interfaces.contains(name) {
        path.kind.segments = qualified_segments(module_path, name);
    } else if let Some(import) = locals.imports.get(name) {
        path.kind.segments = import.path.clone();
    }
}

fn qualify_const_path(
    path: &mut Path,
    module_path: &[String],
    locals: &LocalNames,
    scope: &ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if path.kind.segments.len() != 1 {
        require_explicit_dependency(path, module_path, locals, diagnostics);
        return;
    }
    let name = &path.kind.segments[0];
    if !scope.const_params.contains(name) && locals.consts.contains(name) {
        path.kind.segments = qualified_segments(module_path, name);
    } else if !scope.const_params.contains(name) {
        if let Some(import) = locals.imports.get(name) {
            path.kind.segments = import.path.clone();
        }
    }
}

fn qualify_ctor_path(
    path: &mut Path,
    module_path: &[String],
    locals: &LocalNames,
    scope: &ScopeNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if path.kind.segments.len() != 1 {
        qualify_type_path(path, module_path, locals, scope, diagnostics);
        return;
    }
    let name = &path.kind.segments[0];
    if scope.type_params.contains(name) {
        return;
    }
    if locals.types.contains(name) || locals.interfaces.contains(name) {
        path.kind.segments = qualified_segments(module_path, name);
    } else if let Some(import) = locals.imports.get(name) {
        path.kind.segments = import.path.clone();
    } else if !locals.import_roots.contains(name) {
        diagnostics.push(Diagnostic::error(
            path.span,
            format!("path `{name}::...` requires an explicit `use {name};` dependency"),
        ));
    }
}

fn require_explicit_dependency(
    path: &Path,
    module_path: &[String],
    locals: &LocalNames,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if module_path_has_prefix(path, module_path) {
        return;
    }
    let Some(root) = path.kind.segments.first() else {
        return;
    };
    if locals.import_roots.contains(root) {
        return;
    }
    diagnostics.push(Diagnostic::error(
        path.span,
        format!(
            "path `{}` requires an explicit `use {root};` dependency",
            path_name(path)
        ),
    ));
}

fn module_path_has_prefix(path: &Path, module_path: &[String]) -> bool {
    !module_path.is_empty()
        && path.kind.segments.len() >= module_path.len()
        && path
            .kind
            .segments
            .iter()
            .zip(module_path)
            .all(|(segment, module_segment)| segment == module_segment)
}

fn qualify_name(module_path: &[String], name: &str) -> String {
    qualified_segments(module_path, name).join("::")
}

fn qualified_segments(module_path: &[String], name: &str) -> Vec<String> {
    let mut segments = module_path.to_vec();
    segments.push(name.to_owned());
    segments
}

fn shift_diagnostics(diagnostics: &mut [Diagnostic], base: usize) {
    for diagnostic in diagnostics {
        diagnostic.span = shift_span(diagnostic.span, base);
    }
}

fn shift_span(span: Span, base: usize) -> Span {
    Span::new(
        span.start.saturating_add(base),
        span.end.saturating_add(base),
    )
}

fn shift_program(program: &mut Program, base: usize) {
    program.span = shift_span(program.span, base);
    for item in &mut program.kind.items {
        shift_item(item, base);
    }
}

fn shift_item(item: &mut Item, base: usize) {
    item.span = shift_span(item.span, base);
    match &mut item.kind {
        ItemKind::Use(item) => {
            item.span = shift_span(item.span, base);
            shift_path(&mut item.kind.path, base);
        }
        ItemKind::Const(item) => {
            item.span = shift_span(item.span, base);
            shift_const_expr(&mut item.kind.expr, base);
        }
        ItemKind::Struct(item) => {
            item.span = shift_span(item.span, base);
            shift_generic_params(&mut item.kind.generics, base);
            shift_where_predicates(&mut item.kind.where_predicates, base);
            shift_fields(&mut item.kind.fields, base);
        }
        ItemKind::Enum(item) => {
            item.span = shift_span(item.span, base);
            shift_generic_params(&mut item.kind.generics, base);
            shift_where_predicates(&mut item.kind.where_predicates, base);
            for variant in &mut item.kind.variants {
                variant.span = shift_span(variant.span, base);
                shift_variant_payload(&mut variant.kind.payload, base);
                if let Some(discriminant) = &mut variant.kind.discriminant {
                    shift_const_expr(discriminant, base);
                }
            }
        }
        ItemKind::Interface(item) => {
            item.span = shift_span(item.span, base);
            shift_generic_params(&mut item.kind.generics, base);
            for interface in &mut item.kind.super_interfaces {
                shift_interface_ref(interface, base);
            }
            for member in &mut item.kind.members {
                member.span = shift_span(member.span, base);
                match &mut member.kind {
                    InterfaceMemberKind::AssocConst(sig) => {
                        sig.span = shift_span(sig.span, base);
                        if let Some(default) = &mut sig.kind.default {
                            shift_const_expr(default, base);
                        }
                    }
                    InterfaceMemberKind::Method(sig) => shift_method_sig(sig, base),
                }
            }
        }
        ItemKind::Impl(item) => {
            item.span = shift_span(item.span, base);
            shift_generic_params(&mut item.kind.generics, base);
            shift_where_predicates(&mut item.kind.where_predicates, base);
            if let Some(interface) = &mut item.kind.interface {
                shift_interface_ref(interface, base);
            }
            shift_ty(&mut item.kind.self_ty, base);
            for member in &mut item.kind.members {
                member.span = shift_span(member.span, base);
                match &mut member.kind {
                    ImplMemberKind::AssocConst(item) => {
                        item.span = shift_span(item.span, base);
                        shift_const_expr(&mut item.kind.expr, base);
                    }
                    ImplMemberKind::Method(method) => {
                        method.span = shift_span(method.span, base);
                        shift_method_sig(&mut method.kind.sig, base);
                        shift_expr(&mut method.kind.body, base);
                    }
                }
            }
        }
        ItemKind::Fn(item) => {
            item.span = shift_span(item.span, base);
            shift_generic_params(&mut item.kind.generics, base);
            shift_where_predicates(&mut item.kind.where_predicates, base);
            for param in &mut item.kind.params {
                param.span = shift_span(param.span, base);
                shift_ty(&mut param.kind.ty, base);
            }
            shift_ty(&mut item.kind.ret, base);
            shift_expr(&mut item.kind.body, base);
        }
    }
}

fn shift_generic_params(generics: &mut [GenericParam], base: usize) {
    for generic in generics {
        generic.span = shift_span(generic.span, base);
        if let GenericParamKind::Type { bounds, .. } = &mut generic.kind {
            for bound in bounds {
                shift_interface_ref(bound, base);
            }
        }
    }
}

fn shift_where_predicates(predicates: &mut [WherePredicate], base: usize) {
    for predicate in predicates {
        predicate.span = shift_span(predicate.span, base);
        match &mut predicate.kind {
            WherePredicateKind::Implements { ty, interface } => {
                shift_ty(ty, base);
                shift_interface_ref(interface, base);
            }
            WherePredicateKind::ConstEq { lhs, rhs } => {
                shift_const_expr(lhs, base);
                shift_const_expr(rhs, base);
            }
        }
    }
}

fn shift_interface_ref(interface: &mut InterfaceRef, base: usize) {
    interface.span = shift_span(interface.span, base);
    shift_path(&mut interface.kind.path, base);
    for arg in &mut interface.kind.args {
        shift_generic_arg(arg, base);
    }
}

fn shift_method_sig(sig: &mut MethodSig, base: usize) {
    sig.span = shift_span(sig.span, base);
    shift_generic_params(&mut sig.kind.generics, base);
    if let Some(receiver) = &mut sig.kind.receiver {
        receiver.span = shift_span(receiver.span, base);
    }
    for param in &mut sig.kind.params {
        param.span = shift_span(param.span, base);
        shift_ty(&mut param.kind.ty, base);
    }
    shift_ty(&mut sig.kind.ret, base);
}

fn shift_fields(fields: &mut [Field], base: usize) {
    for field in fields {
        field.span = shift_span(field.span, base);
        shift_ty(&mut field.kind.ty, base);
    }
}

fn shift_variant_payload(payload: &mut VariantPayload, base: usize) {
    payload.span = shift_span(payload.span, base);
    match &mut payload.kind {
        VariantPayloadKind::Unit => {}
        VariantPayloadKind::Tuple(tys) => {
            for ty in tys {
                shift_ty(ty, base);
            }
        }
        VariantPayloadKind::Struct(fields) => shift_fields(fields, base),
    }
}

fn shift_ty(ty: &mut Ty, base: usize) {
    ty.span = shift_span(ty.span, base);
    match &mut ty.kind {
        TyKind::Unit | TyKind::Bool | TyKind::Never | TyKind::Int(_) | TyKind::SelfTy => {}
        TyKind::Path { path, args } => {
            shift_path(path, base);
            for arg in args {
                shift_generic_arg(arg, base);
            }
        }
        TyKind::Array { elem, len } => {
            shift_ty(elem, base);
            shift_const_expr(len, base);
        }
        TyKind::Ref { ty, .. } => shift_ty(ty, base),
        TyKind::Fn { params, ret } => {
            for param in params {
                shift_ty(param, base);
            }
            shift_ty(ret, base);
        }
    }
}

fn shift_generic_arg(arg: &mut GenericArg, base: usize) {
    arg.span = shift_span(arg.span, base);
    match &mut arg.kind {
        GenericArgKind::Ty(ty) => shift_ty(ty, base),
        GenericArgKind::Const(expr) => shift_const_expr(expr, base),
    }
}

fn shift_const_expr(expr: &mut ConstExpr, base: usize) {
    expr.span = shift_span(expr.span, base);
    match &mut expr.kind {
        ConstExprKind::IntLit(_) | ConstExprKind::Param(_) => {}
        ConstExprKind::Path(path) => shift_path(path, base),
        ConstExprKind::AssocConst { ty, interface, .. } => {
            shift_ty(ty, base);
            shift_path(interface, base);
        }
        ConstExprKind::Unary { expr, .. } => shift_const_expr(expr, base),
        ConstExprKind::Binary { lhs, rhs, .. } => {
            shift_const_expr(lhs, base);
            shift_const_expr(rhs, base);
        }
        ConstExprKind::Cast { expr, .. } => shift_const_expr(expr, base),
    }
}

fn shift_expr(expr: &mut Expr, base: usize) {
    expr.span = shift_span(expr.span, base);
    match &mut expr.kind {
        ExprKind::UnitLit
        | ExprKind::BoolLit(_)
        | ExprKind::IntLit(_)
        | ExprKind::Var(_)
        | ExprKind::Todo => {}
        ExprKind::Unary { expr, .. } => shift_expr(expr, base),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Assign { lhs, rhs } => {
            shift_expr(lhs, base);
            shift_expr(rhs, base);
        }
        ExprKind::Call { callee, args } => {
            shift_expr(callee, base);
            for arg in args {
                shift_expr(arg, base);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            shift_expr(receiver, base);
            for arg in args {
                shift_expr(arg, base);
            }
        }
        ExprKind::StructLit { path, fields } => {
            shift_path(path, base);
            shift_field_exprs(fields, base);
        }
        ExprKind::EnumCtor {
            enum_path, args, ..
        } => {
            shift_path(enum_path, base);
            shift_enum_ctor_args(args, base);
        }
        ExprKind::Field { base: expr, .. } => shift_expr(expr, base),
        ExprKind::Index { base: expr, index } => {
            shift_expr(expr, base);
            shift_expr(index, base);
        }
        ExprKind::Match { scrutinee, arms } => {
            shift_expr(scrutinee, base);
            for arm in arms {
                arm.span = shift_span(arm.span, base);
                shift_pat(&mut arm.kind.pat, base);
                shift_expr(&mut arm.kind.body, base);
            }
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            shift_expr(cond, base);
            shift_block(then_branch, base);
            if let Some(block) = else_branch {
                shift_block(block, base);
            }
        }
        ExprKind::Block(block) => shift_block(block, base),
        ExprKind::Return(value) => {
            if let Some(value) = value {
                shift_expr(value, base);
            }
        }
    }
}

fn shift_block(block: &mut Block, base: usize) {
    block.span = shift_span(block.span, base);
    for stmt in &mut block.kind.stmts {
        stmt.span = shift_span(stmt.span, base);
        match &mut stmt.kind {
            StmtKind::Let(let_stmt) => {
                let_stmt.span = shift_span(let_stmt.span, base);
                if let Some(ty) = &mut let_stmt.kind.ty {
                    shift_ty(ty, base);
                }
                if let Some(init) = &mut let_stmt.kind.init {
                    shift_expr(init, base);
                }
            }
            StmtKind::Expr(expr) | StmtKind::Semi(expr) => shift_expr(expr, base),
        }
    }
    if let Some(tail) = &mut block.kind.tail {
        shift_expr(tail, base);
    }
}

fn shift_field_exprs(fields: &mut [FieldExpr], base: usize) {
    for field in fields {
        field.span = shift_span(field.span, base);
        shift_expr(&mut field.kind.expr, base);
    }
}

fn shift_enum_ctor_args(args: &mut EnumCtorArgs, base: usize) {
    args.span = shift_span(args.span, base);
    match &mut args.kind {
        EnumCtorArgsKind::Unit => {}
        EnumCtorArgsKind::Tuple(args) => {
            for arg in args {
                shift_expr(arg, base);
            }
        }
        EnumCtorArgsKind::Struct(fields) => shift_field_exprs(fields, base),
    }
}

fn shift_pat(pat: &mut Pat, base: usize) {
    pat.span = shift_span(pat.span, base);
    match &mut pat.kind {
        PatKind::Wildcard
        | PatKind::Binding { .. }
        | PatKind::Unit
        | PatKind::BoolLit(_)
        | PatKind::IntLit(_) => {}
        PatKind::EnumVariant {
            enum_path, args, ..
        } => {
            shift_path(enum_path, base);
            shift_enum_pat_args(args, base);
        }
        PatKind::Struct { path, fields } => {
            shift_path(path, base);
            shift_field_pats(fields, base);
        }
    }
}

fn shift_enum_pat_args(args: &mut EnumPatArgs, base: usize) {
    args.span = shift_span(args.span, base);
    match &mut args.kind {
        EnumPatArgsKind::Unit => {}
        EnumPatArgsKind::Tuple(pats) => {
            for pat in pats {
                shift_pat(pat, base);
            }
        }
        EnumPatArgsKind::Struct(fields) => shift_field_pats(fields, base),
    }
}

fn shift_field_pats(fields: &mut [FieldPat], base: usize) {
    for field in fields {
        field.span = shift_span(field.span, base);
        shift_pat(&mut field.kind.pat, base);
    }
}

fn shift_path(path: &mut Path, base: usize) {
    path.span = shift_span(path.span, base);
}
