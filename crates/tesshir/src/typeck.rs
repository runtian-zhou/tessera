use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::span::{Node, Span};
use num_bigint::{BigInt, Sign};
use num_traits::{One, ToPrimitive, Zero};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default)]
pub struct TypeCheckReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl TypeCheckReport {
    pub fn is_ok(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

pub fn check_program(program: &Program) -> TypeCheckReport {
    let mut checker = Checker::new(program);
    checker.check();
    TypeCheckReport {
        diagnostics: checker.diagnostics,
    }
}

#[derive(Clone, Debug, Default)]
struct InterfaceReqs {
    methods: HashMap<String, MethodSig>,
    consts: HashMap<String, AssocConstSig>,
}

#[derive(Clone, Debug)]
struct ConstValue {
    value: BigInt,
}

#[derive(Clone, Debug, Default)]
struct GenericScope {
    type_params: HashMap<String, Vec<InterfaceRef>>,
    const_params: HashMap<String, IntTy>,
    obligations: Vec<(Ty, InterfaceRef)>,
}

#[derive(Clone, Debug)]
struct ExprTy {
    ty: Ty,
    diverges: bool,
}

struct Checker<'a> {
    program: &'a Program,
    structs: HashMap<String, &'a StructItem>,
    enums: HashMap<String, &'a EnumItem>,
    interfaces: HashMap<String, &'a InterfaceItem>,
    impls: Vec<&'a ImplItem>,
    fns: HashMap<String, &'a FnItem>,
    const_items: HashMap<String, &'a ConstItem>,
    const_values: HashMap<String, ConstValue>,
    evaluating_consts: HashSet<String>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Checker<'a> {
    fn new(program: &'a Program) -> Self {
        Self {
            program,
            structs: HashMap::new(),
            enums: HashMap::new(),
            interfaces: HashMap::new(),
            impls: vec![],
            fns: HashMap::new(),
            const_items: HashMap::new(),
            const_values: HashMap::new(),
            evaluating_consts: HashSet::new(),
            diagnostics: vec![],
        }
    }

    fn check(&mut self) {
        self.collect_items();
        self.evaluate_all_consts();
        self.check_structs();
        self.check_enums();
        self.check_interfaces();
        self.check_impls();
        self.check_impl_conflicts();
        self.check_fns();
    }

    fn collect_items(&mut self) {
        let mut type_names = HashMap::<String, Span>::new();
        for item in &self.program.kind.items {
            match &item.kind {
                ItemKind::Const(item) => {
                    self.insert_unique_const(&item.kind.name, item.span);
                    self.const_items.insert(item.kind.name.clone(), item);
                }
                ItemKind::Struct(item) => {
                    self.insert_unique_type(&mut type_names, &item.kind.name, item.span);
                    self.structs.insert(item.kind.name.clone(), item);
                }
                ItemKind::Enum(item) => {
                    self.insert_unique_type(&mut type_names, &item.kind.name, item.span);
                    self.enums.insert(item.kind.name.clone(), item);
                }
                ItemKind::Interface(item) => {
                    if self
                        .interfaces
                        .insert(item.kind.name.clone(), item)
                        .is_some()
                    {
                        self.error(
                            item.span,
                            format!("duplicate interface `{}`", item.kind.name),
                        );
                    }
                }
                ItemKind::Impl(item) => self.impls.push(item),
                ItemKind::Fn(item) => {
                    if self.fns.insert(item.kind.name.clone(), item).is_some() {
                        self.error(
                            item.span,
                            format!("duplicate function `{}`", item.kind.name),
                        );
                    }
                }
            }
        }
    }

    fn insert_unique_type(
        &mut self,
        type_names: &mut HashMap<String, Span>,
        name: &str,
        span: Span,
    ) {
        if type_names.insert(name.to_owned(), span).is_some() {
            self.error(span, format!("duplicate type `{name}`"));
        }
    }

    fn insert_unique_const(&mut self, name: &str, span: Span) {
        if self.const_items.contains_key(name) {
            self.error(span, format!("duplicate const `{name}`"));
        }
    }

    fn evaluate_all_consts(&mut self) {
        let mut names: Vec<_> = self.const_items.keys().cloned().collect();
        names.sort();
        for name in names {
            self.eval_top_const(&name);
        }
    }

    fn eval_top_const(&mut self, name: &str) -> Option<ConstValue> {
        if let Some(value) = self.const_values.get(name) {
            return Some(value.clone());
        }
        let item = *self.const_items.get(name)?;
        if self.evaluating_consts.contains(name) {
            self.error(item.span, format!("const cycle involving `{name}`"));
            return None;
        }
        self.evaluating_consts.insert(name.to_owned());
        let scope = GenericScope::default();
        let value = self.eval_const_expr(&item.kind.expr, &scope)?;
        self.evaluating_consts.remove(name);
        if !fits_int_ty(&value, item.kind.ty) {
            self.error(
                item.kind.expr.span,
                format!(
                    "const `{name}` value `{value}` does not fit in {}",
                    item.kind.ty
                ),
            );
            return None;
        }
        let const_value = ConstValue { value };
        self.const_values
            .insert(name.to_owned(), const_value.clone());
        Some(const_value)
    }

    fn check_structs(&mut self) {
        let mut items = self.structs.values().copied().collect::<Vec<_>>();
        items.sort_by(|a, b| a.kind.name.cmp(&b.kind.name));
        for item in items {
            let scope = self.scope_from_generics(&item.kind.generics, &item.kind.where_predicates);
            self.check_generic_param_names(&item.kind.generics);
            self.check_duplicate_fields(&item.kind.fields);
            for field in &item.kind.fields {
                self.check_ty_wf(&field.kind.ty, &scope, false);
            }
            self.check_where_predicates(&item.kind.where_predicates, &scope);
        }
    }

    fn check_enums(&mut self) {
        let mut items = self.enums.values().copied().collect::<Vec<_>>();
        items.sort_by(|a, b| a.kind.name.cmp(&b.kind.name));
        for item in items {
            let scope = self.scope_from_generics(&item.kind.generics, &item.kind.where_predicates);
            self.check_generic_param_names(&item.kind.generics);
            self.check_where_predicates(&item.kind.where_predicates, &scope);
            let mut variants = HashSet::new();
            let mut discriminants = HashSet::<BigInt>::new();
            for variant in &item.kind.variants {
                if !variants.insert(variant.kind.name.clone()) {
                    self.error(
                        variant.span,
                        format!("duplicate enum variant `{}`", variant.kind.name),
                    );
                }
                match &variant.kind.payload.kind {
                    VariantPayloadKind::Unit => {}
                    VariantPayloadKind::Tuple(tys) => {
                        for ty in tys {
                            self.check_ty_wf(ty, &scope, false);
                        }
                    }
                    VariantPayloadKind::Struct(fields) => {
                        self.check_duplicate_fields(fields);
                        for field in fields {
                            self.check_ty_wf(&field.kind.ty, &scope, false);
                        }
                    }
                }
                if let Some(discriminant) = &variant.kind.discriminant {
                    if !matches!(variant.kind.payload.kind, VariantPayloadKind::Unit) {
                        self.error(
                            discriminant.span,
                            "only unit enum variants may have explicit discriminants",
                        );
                    }
                    if let Some(value) = self.eval_const_expr(discriminant, &scope) {
                        if let Some(repr) = item.kind.repr {
                            if !fits_int_ty(&value, repr) {
                                self.error(
                                    discriminant.span,
                                    format!("discriminant `{value}` does not fit in {repr}"),
                                );
                            }
                        }
                        if !discriminants.insert(value.clone()) {
                            self.error(
                                discriminant.span,
                                format!("duplicate enum discriminant `{value}`"),
                            );
                        }
                    }
                }
            }
        }
    }

    fn check_interfaces(&mut self) {
        let mut items = self.interfaces.values().copied().collect::<Vec<_>>();
        items.sort_by(|a, b| a.kind.name.cmp(&b.kind.name));
        for item in items {
            let mut scope = self.scope_from_generics(&item.kind.generics, &[]);
            scope.obligations.push((
                Node::new(item.span, TyKind::SelfTy),
                self.current_interface_ref(item),
            ));
            self.check_generic_param_names(&item.kind.generics);
            let mut names = HashSet::new();
            for super_interface in &item.kind.super_interfaces {
                self.check_interface_ref(super_interface, &scope);
            }
            for member in &item.kind.members {
                match &member.kind {
                    InterfaceMemberKind::AssocConst(sig) => {
                        if !names.insert(sig.kind.name.clone()) {
                            self.error(
                                sig.span,
                                format!("duplicate interface member `{}`", sig.kind.name),
                            );
                        }
                        if let Some(default) = &sig.kind.default {
                            if let Some(value) = self.eval_const_expr(default, &scope) {
                                if !fits_int_ty(&value, sig.kind.ty) {
                                    self.error(
                                        default.span,
                                        format!(
                                            "associated const default `{value}` does not fit in {}",
                                            sig.kind.ty
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    InterfaceMemberKind::Method(sig) => {
                        if !names.insert(sig.kind.name.clone()) {
                            self.error(
                                sig.span,
                                format!("duplicate interface member `{}`", sig.kind.name),
                            );
                        }
                        self.check_method_sig(sig, &scope, true);
                    }
                }
            }
            let interface_ref = self.current_interface_ref(item);
            let mut stack = vec![];
            self.interface_reqs_checked(&interface_ref, &mut stack, &scope);
        }
    }

    fn check_impls(&mut self) {
        for item in self.impls.clone() {
            let scope = self.scope_from_generics(&item.kind.generics, &item.kind.where_predicates);
            self.check_generic_param_names(&item.kind.generics);
            self.check_where_predicates(&item.kind.where_predicates, &scope);
            self.check_ty_wf(&item.kind.self_ty, &scope, false);
            if let Some(interface_ref) = &item.kind.interface {
                self.check_interface_ref(interface_ref, &scope);
                self.check_interface_impl(item, interface_ref, &scope);
            }
            for member in &item.kind.members {
                match &member.kind {
                    ImplMemberKind::AssocConst(c) => {
                        self.check_assoc_const_impl_expr(c, &scope);
                    }
                    ImplMemberKind::Method(m) => {
                        self.check_method_sig(&m.kind.sig, &scope, true);
                        self.check_method_body(m, &item.kind.self_ty, &scope);
                    }
                }
            }
        }
    }

    fn check_interface_impl(
        &mut self,
        item: &ImplItem,
        interface_ref: &InterfaceRef,
        scope: &GenericScope,
    ) {
        let Some(reqs) = self.interface_reqs(interface_ref) else {
            return;
        };
        let mut const_reqs = reqs.consts.iter().collect::<Vec<_>>();
        const_reqs.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (name, sig) in const_reqs {
            let provided = item
                .kind
                .members
                .iter()
                .find_map(|member| match &member.kind {
                    ImplMemberKind::AssocConst(c) if &c.kind.name == name => Some(c),
                    _ => None,
                });
            match provided {
                Some(provided) => {
                    if provided.kind.ty != sig.kind.ty {
                        self.error(
                            provided.span,
                            format!(
                                "associated const `{name}` has type `{}`, expected `{}`",
                                provided.kind.ty, sig.kind.ty
                            ),
                        );
                    }
                }
                None if sig.kind.default.is_none() => {
                    self.error(
                        item.span,
                        format!(
                            "impl for `{}` is missing associated const `{name}`",
                            path_name(&interface_ref.kind.path)
                        ),
                    );
                }
                None => {}
            }
        }
        let mut method_reqs = reqs.methods.iter().collect::<Vec<_>>();
        method_reqs.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (name, required) in method_reqs {
            let provided = item
                .kind
                .members
                .iter()
                .find_map(|member| match &member.kind {
                    ImplMemberKind::Method(m) if &m.kind.sig.kind.name == name => Some(&m.kind.sig),
                    _ => None,
                });
            let Some(provided) = provided else {
                self.error(
                    item.span,
                    format!(
                        "impl for `{}` is missing method `{name}`",
                        path_name(&interface_ref.kind.path)
                    ),
                );
                continue;
            };
            if !self.method_sigs_match(required, provided, &item.kind.self_ty, scope) {
                self.error(
                    provided.span,
                    format!("method `{name}` signature does not match interface requirement"),
                );
            }
        }
    }

    fn check_assoc_const_impl_expr(&mut self, item: &AssocConstImpl, scope: &GenericScope) {
        if let Some(value) = self.eval_const_expr(&item.kind.expr, scope) {
            if !fits_int_ty(&value, item.kind.ty) {
                self.error(
                    item.kind.expr.span,
                    format!(
                        "associated const `{}` value `{value}` does not fit in {}",
                        item.kind.name, item.kind.ty
                    ),
                );
            }
        }
    }

    fn check_impl_conflicts(&mut self) {
        for i in 0..self.impls.len() {
            let a = self.impls[i];
            let Some(a_interface) = &a.kind.interface else {
                continue;
            };
            for j in (i + 1)..self.impls.len() {
                let b = self.impls[j];
                let Some(b_interface) = &b.kind.interface else {
                    continue;
                };
                if path_name(&a_interface.kind.path) != path_name(&b_interface.kind.path) {
                    continue;
                }
                let scope = GenericScope::default();
                if self.ty_eq_no_diag(&a.kind.self_ty, &b.kind.self_ty, &scope) {
                    self.error(
                        b.span,
                        format!(
                            "conflicting impl of `{}` for `{}`",
                            path_name(&a_interface.kind.path),
                            self.ty_display(&b.kind.self_ty)
                        ),
                    );
                }
            }
        }
    }

    fn check_fns(&mut self) {
        let mut items = self.fns.values().copied().collect::<Vec<_>>();
        items.sort_by(|a, b| a.kind.name.cmp(&b.kind.name));
        for item in items {
            let scope = self.scope_from_generics(&item.kind.generics, &item.kind.where_predicates);
            self.check_generic_param_names(&item.kind.generics);
            self.check_where_predicates(&item.kind.where_predicates, &scope);
            let mut locals = HashMap::new();
            for param in &item.kind.params {
                self.check_ty_wf(&param.kind.ty, &scope, false);
                locals.insert(param.kind.name.clone(), param.kind.ty.clone());
            }
            self.check_ty_wf(&item.kind.ret, &scope, false);
            let actual = self.check_expr(&item.kind.body, &scope, &mut locals, &item.kind.ret);
            if !actual.diverges && !self.ty_eq(&actual.ty, &item.kind.ret, &scope) {
                self.error(
                    item.kind.body.span,
                    format!(
                        "function `{}` returns `{}`, expected `{}`",
                        item.kind.name,
                        self.ty_display(&actual.ty),
                        self.ty_display(&item.kind.ret)
                    ),
                );
            }
        }
    }

    fn check_method_sig(&mut self, sig: &MethodSig, scope: &GenericScope, allow_self: bool) {
        self.check_generic_param_names(&sig.kind.generics);
        let mut method_scope = scope.clone();
        self.extend_scope_with_generics(&mut method_scope, &sig.kind.generics);
        for param in &sig.kind.params {
            self.check_ty_wf(&param.kind.ty, &method_scope, allow_self);
        }
        self.check_ty_wf(&sig.kind.ret, &method_scope, allow_self);
    }

    fn check_method_body(&mut self, method: &MethodDef, self_ty: &Ty, scope: &GenericScope) {
        let mut locals = HashMap::new();
        if let Some(receiver) = &method.kind.sig.kind.receiver {
            let ty = match receiver.kind {
                ReceiverKind::ByValue => self_ty.clone(),
                ReceiverKind::ByRef { mutability } => Node::new(
                    receiver.span,
                    TyKind::Ref {
                        mutability,
                        ty: Box::new(self_ty.clone()),
                    },
                ),
            };
            locals.insert("self".to_owned(), ty);
        }
        for param in &method.kind.sig.kind.params {
            locals.insert(
                param.kind.name.clone(),
                self.substitute_self(&param.kind.ty, self_ty),
            );
        }
        let ret = self.substitute_self(&method.kind.sig.kind.ret, self_ty);
        let actual = self.check_expr(&method.kind.body, scope, &mut locals, &ret);
        if !actual.diverges && !self.ty_eq(&actual.ty, &ret, scope) {
            self.error(
                method.kind.body.span,
                format!(
                    "method `{}` returns `{}`, expected `{}`",
                    method.kind.sig.kind.name,
                    self.ty_display(&actual.ty),
                    self.ty_display(&ret)
                ),
            );
        }
    }

    fn check_expr(
        &mut self,
        expr: &Expr,
        scope: &GenericScope,
        locals: &mut HashMap<String, Ty>,
        expected_ret: &Ty,
    ) -> ExprTy {
        match &expr.kind {
            ExprKind::UnitLit => ExprTy {
                ty: Node::new(expr.span, TyKind::Unit),
                diverges: false,
            },
            ExprKind::BoolLit(_) => ExprTy {
                ty: Node::new(expr.span, TyKind::Bool),
                diverges: false,
            },
            ExprKind::IntLit(value) => {
                let int_ty = match expected_ret.kind {
                    TyKind::Int(ty) => ty,
                    _ => IntTy::i32(),
                };
                if !fits_int_ty(value, int_ty) {
                    self.error(
                        expr.span,
                        format!("integer literal does not fit in {int_ty}"),
                    );
                }
                ExprTy {
                    ty: Node::new(expr.span, TyKind::Int(int_ty)),
                    diverges: false,
                }
            }
            ExprKind::Var(name) => {
                if let Some(ty) = locals.get(name) {
                    ExprTy {
                        ty: ty.clone(),
                        diverges: false,
                    }
                } else {
                    self.error(expr.span, format!("unknown variable `{name}`"));
                    self.error_ty(expr.span)
                }
            }
            ExprKind::Unary { op, expr: inner } => {
                let inner_ty = self.check_expr(inner, scope, locals, expected_ret);
                match op {
                    UnaryOp::Neg => {
                        if !matches!(inner_ty.ty.kind, TyKind::Int(_)) {
                            self.error(expr.span, "unary `-` expects integer operand");
                        }
                        inner_ty
                    }
                    UnaryOp::Not => {
                        if !matches!(inner_ty.ty.kind, TyKind::Bool) {
                            self.error(expr.span, "unary `!` expects bool operand");
                        }
                        ExprTy {
                            ty: Node::new(expr.span, TyKind::Bool),
                            diverges: inner_ty.diverges,
                        }
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lhs_ty = self.check_expr(lhs, scope, locals, expected_ret);
                let rhs_ty = self.check_expr(rhs, scope, locals, expected_ret);
                match op {
                    BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Rem
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr => {
                        if !self.ty_eq(&lhs_ty.ty, &rhs_ty.ty, scope)
                            || !matches!(lhs_ty.ty.kind, TyKind::Int(_))
                        {
                            self.error(
                                expr.span,
                                "binary arithmetic expects matching integer operands",
                            );
                        }
                        ExprTy {
                            ty: lhs_ty.ty,
                            diverges: lhs_ty.diverges || rhs_ty.diverges,
                        }
                    }
                    BinaryOp::Eq
                    | BinaryOp::Ne
                    | BinaryOp::Lt
                    | BinaryOp::Le
                    | BinaryOp::Gt
                    | BinaryOp::Ge => {
                        if !self.ty_eq(&lhs_ty.ty, &rhs_ty.ty, scope) {
                            self.error(expr.span, "comparison expects matching operand types");
                        }
                        ExprTy {
                            ty: Node::new(expr.span, TyKind::Bool),
                            diverges: lhs_ty.diverges || rhs_ty.diverges,
                        }
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        if !matches!(lhs_ty.ty.kind, TyKind::Bool)
                            || !matches!(rhs_ty.ty.kind, TyKind::Bool)
                        {
                            self.error(expr.span, "logical operator expects bool operands");
                        }
                        ExprTy {
                            ty: Node::new(expr.span, TyKind::Bool),
                            diverges: lhs_ty.diverges || rhs_ty.diverges,
                        }
                    }
                }
            }
            ExprKind::Call { callee, args } => {
                self.check_call(expr.span, callee, args, scope, locals, expected_ret)
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.check_method_call(
                expr.span,
                receiver,
                method,
                args,
                scope,
                locals,
                expected_ret,
            ),
            ExprKind::StructLit { path, fields } => {
                let Some(struct_item) = self.structs.get(&path_name(path)).copied() else {
                    self.error(path.span, format!("unknown struct `{}`", path_name(path)));
                    return self.error_ty(expr.span);
                };
                let mut seen_fields = HashSet::new();
                for field in fields {
                    if !seen_fields.insert(field.kind.name.clone()) {
                        self.error(field.span, format!("duplicate field `{}`", field.kind.name));
                        continue;
                    }
                    let Some(expected) = struct_item
                        .kind
                        .fields
                        .iter()
                        .find(|decl| decl.kind.name == field.kind.name)
                    else {
                        self.error(field.span, format!("unknown field `{}`", field.kind.name));
                        continue;
                    };
                    let actual =
                        self.check_expr(&field.kind.expr, scope, locals, &expected.kind.ty);
                    if !self.ty_eq(&actual.ty, &expected.kind.ty, scope) {
                        self.error(
                            field.kind.expr.span,
                            format!(
                                "field `{}` has type `{}`, expected `{}`",
                                field.kind.name,
                                self.ty_display(&actual.ty),
                                self.ty_display(&expected.kind.ty)
                            ),
                        );
                    }
                }
                for expected in &struct_item.kind.fields {
                    if !seen_fields.contains(&expected.kind.name) {
                        self.error(
                            expr.span,
                            format!(
                                "missing field `{}` for struct `{}`",
                                expected.kind.name, struct_item.kind.name
                            ),
                        );
                    }
                }
                ExprTy {
                    ty: Node::new(
                        expr.span,
                        TyKind::Path {
                            path: path.clone(),
                            args: vec![],
                        },
                    ),
                    diverges: false,
                }
            }
            ExprKind::EnumCtor {
                enum_path,
                variant,
                args,
            } => self.check_enum_ctor(
                expr.span,
                enum_path,
                variant,
                args,
                scope,
                locals,
                expected_ret,
            ),
            ExprKind::Field { base, name } => {
                let base_ty = self.check_expr(base, scope, locals, expected_ret);
                let Some(field_ty) = self.lookup_field_type(&base_ty.ty, name) else {
                    self.error(
                        expr.span,
                        format!(
                            "type `{}` has no field `{name}`",
                            self.ty_display(&base_ty.ty)
                        ),
                    );
                    return self.error_ty(expr.span);
                };
                ExprTy {
                    ty: field_ty,
                    diverges: base_ty.diverges,
                }
            }
            ExprKind::Index { base, index } => {
                let base_ty = self.check_expr(base, scope, locals, expected_ret);
                let index_ty = self.check_expr(index, scope, locals, expected_ret);
                if !matches!(index_ty.ty.kind, TyKind::Int(_)) {
                    self.error(index.span, "array index must be an integer");
                }
                match &base_ty.ty.kind {
                    TyKind::Array { elem, .. } => ExprTy {
                        ty: *elem.clone(),
                        diverges: base_ty.diverges || index_ty.diverges,
                    },
                    _ => {
                        self.error(base.span, "indexing requires an array value");
                        self.error_ty(expr.span)
                    }
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.check_match(expr.span, scrutinee, arms, scope, locals, expected_ret)
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.check_expr(cond, scope, locals, expected_ret);
                if !matches!(cond_ty.ty.kind, TyKind::Bool) {
                    self.error(cond.span, "if condition must be bool");
                }
                let then_ty = self.check_block(then_branch, scope, locals, expected_ret);
                let else_ty = if let Some(block) = else_branch {
                    self.check_block(block, scope, locals, expected_ret)
                } else {
                    ExprTy {
                        ty: Node::new(expr.span, TyKind::Unit),
                        diverges: false,
                    }
                };
                if !then_ty.diverges
                    && !else_ty.diverges
                    && !self.ty_eq(&then_ty.ty, &else_ty.ty, scope)
                {
                    self.error(expr.span, "if branches must have the same type");
                }
                ExprTy {
                    ty: if then_ty.diverges {
                        else_ty.ty
                    } else {
                        then_ty.ty
                    },
                    diverges: then_ty.diverges && else_ty.diverges,
                }
            }
            ExprKind::Block(block) => self.check_block(block, scope, locals, expected_ret),
            ExprKind::Return(value) => {
                let value_ty = if let Some(value) = value {
                    self.check_expr(value, scope, locals, expected_ret).ty
                } else {
                    Node::new(expr.span, TyKind::Unit)
                };
                if !self.ty_eq(&value_ty, expected_ret, scope) {
                    self.error(
                        expr.span,
                        format!(
                            "return has type `{}`, expected `{}`",
                            self.ty_display(&value_ty),
                            self.ty_display(expected_ret)
                        ),
                    );
                }
                ExprTy {
                    ty: Node::new(expr.span, TyKind::Never),
                    diverges: true,
                }
            }
            ExprKind::Assign { lhs, rhs } => {
                let lhs_ty = self.check_expr(lhs, scope, locals, expected_ret);
                let rhs_ty = self.check_expr(rhs, scope, locals, expected_ret);
                if !self.ty_eq(&lhs_ty.ty, &rhs_ty.ty, scope) {
                    self.error(expr.span, "assignment types do not match");
                }
                ExprTy {
                    ty: Node::new(expr.span, TyKind::Unit),
                    diverges: lhs_ty.diverges || rhs_ty.diverges,
                }
            }
            ExprKind::Todo => ExprTy {
                ty: Node::new(expr.span, TyKind::Never),
                diverges: true,
            },
        }
    }

    fn check_call(
        &mut self,
        span: Span,
        callee: &Expr,
        args: &[Expr],
        scope: &GenericScope,
        locals: &mut HashMap<String, Ty>,
        expected_ret: &Ty,
    ) -> ExprTy {
        if let ExprKind::Var(name) = &callee.kind {
            if let Some(fn_item) = self.fns.get(name).copied() {
                if fn_item.kind.params.len() != args.len() {
                    self.error(
                        span,
                        format!(
                            "function `{name}` expects {} arguments",
                            fn_item.kind.params.len()
                        ),
                    );
                }
                for (arg, param) in args.iter().zip(&fn_item.kind.params) {
                    let actual = self.check_expr(arg, scope, locals, &param.kind.ty);
                    if !self.ty_eq(&actual.ty, &param.kind.ty, scope) {
                        self.error(
                            arg.span,
                            format!(
                                "argument has type `{}`, expected `{}`",
                                self.ty_display(&actual.ty),
                                self.ty_display(&param.kind.ty)
                            ),
                        );
                    }
                }
                return ExprTy {
                    ty: fn_item.kind.ret.clone(),
                    diverges: false,
                };
            }
        }
        let callee_ty = self.check_expr(callee, scope, locals, expected_ret);
        match &callee_ty.ty.kind {
            TyKind::Fn { params, ret } => {
                if params.len() != args.len() {
                    self.error(
                        span,
                        format!("function value expects {} arguments", params.len()),
                    );
                }
                for (arg, expected) in args.iter().zip(params) {
                    let actual = self.check_expr(arg, scope, locals, expected);
                    if !self.ty_eq(&actual.ty, expected, scope) {
                        self.error(arg.span, "function argument type mismatch");
                    }
                }
                ExprTy {
                    ty: *ret.clone(),
                    diverges: false,
                }
            }
            _ => {
                self.error(callee.span, "callee is not a function");
                self.error_ty(span)
            }
        }
    }

    fn check_method_call(
        &mut self,
        span: Span,
        receiver: &Expr,
        method: &str,
        args: &[Expr],
        scope: &GenericScope,
        locals: &mut HashMap<String, Ty>,
        expected_ret: &Ty,
    ) -> ExprTy {
        let receiver_ty = self.check_expr(receiver, scope, locals, expected_ret).ty;
        let self_ty = peel_ref(&receiver_ty).unwrap_or(&receiver_ty).clone();
        let Some((sig, impl_self_ty)) = self.find_method(&self_ty, method, scope) else {
            self.error(
                span,
                format!(
                    "no method `{method}` found for `{}`",
                    self.ty_display(&self_ty)
                ),
            );
            return self.error_ty(span);
        };
        if let Some(recv) = &sig.kind.receiver {
            let expected_receiver = match recv.kind {
                ReceiverKind::ByValue => impl_self_ty.clone(),
                ReceiverKind::ByRef { mutability } => Node::new(
                    receiver_ty.span,
                    TyKind::Ref {
                        mutability,
                        ty: Box::new(impl_self_ty.clone()),
                    },
                ),
            };
            if !self.ty_eq(&receiver_ty, &expected_receiver, scope) {
                self.error(
                    receiver.span,
                    format!(
                        "method receiver has type `{}`, expected `{}`",
                        self.ty_display(&receiver_ty),
                        self.ty_display(&expected_receiver)
                    ),
                );
            }
        }
        if sig.kind.params.len() != args.len() {
            self.error(
                span,
                format!(
                    "method `{method}` expects {} arguments",
                    sig.kind.params.len()
                ),
            );
        }
        for (arg, param) in args.iter().zip(&sig.kind.params) {
            let expected = self.substitute_self(&param.kind.ty, &impl_self_ty);
            let actual = self.check_expr(arg, scope, locals, &expected);
            if !self.ty_eq(&actual.ty, &expected, scope) {
                self.error(
                    arg.span,
                    format!(
                        "argument has type `{}`, expected `{}`",
                        self.ty_display(&actual.ty),
                        self.ty_display(&expected)
                    ),
                );
            }
        }
        ExprTy {
            ty: self.substitute_self(&sig.kind.ret, &impl_self_ty),
            diverges: false,
        }
    }

    fn find_method(
        &self,
        self_ty: &Ty,
        method: &str,
        scope: &GenericScope,
    ) -> Option<(MethodSig, Ty)> {
        for (ty, interface_ref) in &scope.obligations {
            if self.ty_eq_no_diag(self_ty, ty, scope) {
                if let Some(reqs) = self.interface_reqs(interface_ref) {
                    if let Some(sig) = reqs.methods.get(method) {
                        return Some((sig.clone(), ty.clone()));
                    }
                }
            }
        }
        for imp in &self.impls {
            if !self.ty_eq_no_diag(self_ty, &imp.kind.self_ty, scope) {
                continue;
            }
            if let Some(interface_ref) = &imp.kind.interface {
                if let Some(reqs) = self.interface_reqs(interface_ref) {
                    if let Some(sig) = reqs.methods.get(method) {
                        return Some((sig.clone(), imp.kind.self_ty.clone()));
                    }
                }
            }
            for member in &imp.kind.members {
                if let ImplMemberKind::Method(method_def) = &member.kind {
                    if method_def.kind.sig.kind.name == method {
                        return Some((method_def.kind.sig.clone(), imp.kind.self_ty.clone()));
                    }
                }
            }
        }
        None
    }

    fn check_enum_ctor(
        &mut self,
        span: Span,
        enum_path: &Path,
        variant: &str,
        args: &EnumCtorArgs,
        scope: &GenericScope,
        locals: &mut HashMap<String, Ty>,
        expected_ret: &Ty,
    ) -> ExprTy {
        let enum_name = path_name(enum_path);
        let Some(enum_item) = self.enums.get(&enum_name).copied() else {
            self.error(enum_path.span, format!("unknown enum `{enum_name}`"));
            return self.error_ty(span);
        };
        let Some(variant_decl) = enum_item
            .kind
            .variants
            .iter()
            .find(|decl| decl.kind.name == variant)
        else {
            self.error(
                span,
                format!("unknown variant `{variant}` for enum `{enum_name}`"),
            );
            return self.error_ty(span);
        };
        let mut substitutions = HashMap::<String, Ty>::new();
        match (&variant_decl.kind.payload.kind, &args.kind) {
            (VariantPayloadKind::Unit, EnumCtorArgsKind::Unit) => {}
            (VariantPayloadKind::Tuple(expected), EnumCtorArgsKind::Tuple(actual)) => {
                if expected.len() != actual.len() {
                    self.error(
                        span,
                        format!("variant `{variant}` expects {} arguments", expected.len()),
                    );
                }
                for (expected, actual_expr) in expected.iter().zip(actual) {
                    let actual_ty = self.check_expr(actual_expr, scope, locals, expected_ret).ty;
                    self.unify_variant_ty(
                        expected,
                        &actual_ty,
                        &mut substitutions,
                        scope,
                        actual_expr.span,
                    );
                }
            }
            (VariantPayloadKind::Struct(expected), EnumCtorArgsKind::Struct(actual)) => {
                let mut seen_fields = HashSet::new();
                for actual_field in actual {
                    if !seen_fields.insert(actual_field.kind.name.clone()) {
                        self.error(
                            actual_field.span,
                            format!(
                                "duplicate field `{}` for variant `{variant}`",
                                actual_field.kind.name
                            ),
                        );
                        continue;
                    }
                    if !expected
                        .iter()
                        .any(|field| field.kind.name == actual_field.kind.name)
                    {
                        self.error(
                            actual_field.span,
                            format!(
                                "unknown field `{}` for variant `{variant}`",
                                actual_field.kind.name
                            ),
                        );
                    }
                }
                for expected_field in expected {
                    let Some(actual_field) = actual
                        .iter()
                        .find(|field| field.kind.name == expected_field.kind.name)
                    else {
                        self.error(
                            span,
                            format!(
                                "missing field `{}` for variant `{variant}`",
                                expected_field.kind.name
                            ),
                        );
                        continue;
                    };
                    let actual_ty = self
                        .check_expr(
                            &actual_field.kind.expr,
                            scope,
                            locals,
                            &expected_field.kind.ty,
                        )
                        .ty;
                    self.unify_variant_ty(
                        &expected_field.kind.ty,
                        &actual_ty,
                        &mut substitutions,
                        scope,
                        actual_field.span,
                    );
                }
            }
            _ => self.error(
                span,
                format!("variant `{variant}` payload form does not match constructor"),
            ),
        }
        let mut generic_args = vec![];
        for generic in &enum_item.kind.generics {
            match &generic.kind {
                GenericParamKind::Type { name, .. } => {
                    let ty = substitutions.get(name).cloned().unwrap_or_else(|| {
                        Node::new(
                            span,
                            TyKind::Path {
                                path: synthetic_path(name),
                                args: vec![],
                            },
                        )
                    });
                    generic_args.push(Node::new(ty.span, GenericArgKind::Ty(ty)));
                }
                GenericParamKind::Const { name, .. } => {
                    generic_args.push(Node::new(
                        span,
                        GenericArgKind::Const(Node::new(span, ConstExprKind::Param(name.clone()))),
                    ));
                }
            }
        }
        ExprTy {
            ty: Node::new(
                span,
                TyKind::Path {
                    path: enum_path.clone(),
                    args: generic_args,
                },
            ),
            diverges: false,
        }
    }

    fn unify_variant_ty(
        &mut self,
        expected: &Ty,
        actual: &Ty,
        substitutions: &mut HashMap<String, Ty>,
        scope: &GenericScope,
        span: Span,
    ) {
        if let TyKind::Path { path, args } = &expected.kind {
            if path.kind.segments.len() == 1 && args.is_empty() {
                let name = &path.kind.segments[0];
                if scope.type_params.contains_key(name) || substitutions.contains_key(name) {
                    if let Some(existing) = substitutions.get(name) {
                        if !self.ty_eq(existing, actual, scope) {
                            self.error(
                                span,
                                format!("conflicting inference for type parameter `{name}`"),
                            );
                        }
                    } else {
                        substitutions.insert(name.clone(), actual.clone());
                    }
                    return;
                }
            }
        }
        if !self.ty_eq(expected, actual, scope) {
            self.error(
                span,
                format!(
                    "constructor argument has type `{}`, expected `{}`",
                    self.ty_display(actual),
                    self.ty_display(expected)
                ),
            );
        }
    }

    fn check_match(
        &mut self,
        span: Span,
        scrutinee: &Expr,
        arms: &[MatchArm],
        scope: &GenericScope,
        locals: &mut HashMap<String, Ty>,
        expected_ret: &Ty,
    ) -> ExprTy {
        let scrutinee_ty = self.check_expr(scrutinee, scope, locals, expected_ret).ty;
        let mut result_ty = None::<Ty>;
        let mut seen_variants = HashSet::new();
        let mut has_wildcard = false;
        for arm in arms {
            let mut arm_locals = locals.clone();
            if self.check_pat(
                &arm.kind.pat,
                &scrutinee_ty,
                scope,
                &mut arm_locals,
                &mut seen_variants,
            ) {
                has_wildcard = true;
            }
            let body_ty = self.check_expr(&arm.kind.body, scope, &mut arm_locals, expected_ret);
            if let Some(existing) = &result_ty {
                if !body_ty.diverges && !self.ty_eq(existing, &body_ty.ty, scope) {
                    self.error(arm.kind.body.span, "match arms must have the same type");
                }
            } else if !body_ty.diverges {
                result_ty = Some(body_ty.ty);
            }
        }
        self.check_match_exhaustive(span, &scrutinee_ty, &seen_variants, has_wildcard);
        ExprTy {
            ty: result_ty.unwrap_or_else(|| Node::new(span, TyKind::Never)),
            diverges: false,
        }
    }

    fn check_pat(
        &mut self,
        pat: &Pat,
        expected: &Ty,
        scope: &GenericScope,
        locals: &mut HashMap<String, Ty>,
        seen_variants: &mut HashSet<String>,
    ) -> bool {
        match &pat.kind {
            PatKind::Wildcard => true,
            PatKind::Binding { name } => {
                locals.insert(name.clone(), expected.clone());
                true
            }
            PatKind::Unit => {
                if !matches!(expected.kind, TyKind::Unit) {
                    self.error(pat.span, "unit pattern applied to non-unit value");
                }
                false
            }
            PatKind::BoolLit(_) => {
                if !matches!(expected.kind, TyKind::Bool) {
                    self.error(pat.span, "bool pattern applied to non-bool value");
                }
                false
            }
            PatKind::IntLit(_) => {
                if !matches!(expected.kind, TyKind::Int(_)) {
                    self.error(pat.span, "integer pattern applied to non-integer value");
                }
                false
            }
            PatKind::EnumVariant {
                enum_path,
                variant,
                args,
            } => {
                let enum_name = path_name(enum_path);
                let expected_enum = match &expected.kind {
                    TyKind::Path { path, args } if path_name(path) == enum_name => args,
                    _ => {
                        self.error(
                            pat.span,
                            "enum variant pattern applied to non-matching enum type",
                        );
                        return false;
                    }
                };
                let Some(enum_item) = self.enums.get(&enum_name).copied() else {
                    self.error(enum_path.span, format!("unknown enum `{enum_name}`"));
                    return false;
                };
                let Some(variant_decl) = enum_item
                    .kind
                    .variants
                    .iter()
                    .find(|v| v.kind.name == *variant)
                else {
                    self.error(pat.span, format!("unknown variant `{variant}`"));
                    return false;
                };
                seen_variants.insert(variant.clone());
                let substitutions =
                    self.generic_arg_substitutions(&enum_item.kind.generics, expected_enum);
                match (&variant_decl.kind.payload.kind, &args.kind) {
                    (VariantPayloadKind::Unit, EnumPatArgsKind::Unit) => {}
                    (VariantPayloadKind::Tuple(tys), EnumPatArgsKind::Tuple(pats)) => {
                        if tys.len() != pats.len() {
                            self.error(
                                pat.span,
                                format!("variant `{variant}` expects {} fields", tys.len()),
                            );
                        }
                        for (ty, pat) in tys.iter().zip(pats) {
                            let ty = self.apply_type_substitutions(ty, &substitutions);
                            let _ = self.check_pat(pat, &ty, scope, locals, seen_variants);
                        }
                    }
                    (VariantPayloadKind::Struct(fields), EnumPatArgsKind::Struct(pats)) => {
                        for field in fields {
                            let Some(field_pat) =
                                pats.iter().find(|p| p.kind.name == field.kind.name)
                            else {
                                self.error(
                                    pat.span,
                                    format!("missing pattern field `{}`", field.kind.name),
                                );
                                continue;
                            };
                            let ty = self.apply_type_substitutions(&field.kind.ty, &substitutions);
                            let _ = self.check_pat(
                                &field_pat.kind.pat,
                                &ty,
                                scope,
                                locals,
                                seen_variants,
                            );
                        }
                    }
                    _ => self.error(pat.span, "enum pattern payload form does not match variant"),
                }
                false
            }
            PatKind::Struct { path, fields } => {
                let struct_name = path_name(path);
                let expected_args = match &expected.kind {
                    TyKind::Path { path, args } if path_name(path) == struct_name => args,
                    _ => {
                        self.error(
                            pat.span,
                            "struct pattern applied to non-matching struct type",
                        );
                        return false;
                    }
                };
                let Some(struct_item) = self.structs.get(&struct_name).copied() else {
                    self.error(path.span, format!("unknown struct `{struct_name}`"));
                    return false;
                };
                let substitutions =
                    self.generic_arg_substitutions(&struct_item.kind.generics, expected_args);
                let mut seen_fields = HashSet::new();
                for field_pat in fields {
                    if !seen_fields.insert(field_pat.kind.name.clone()) {
                        self.error(
                            field_pat.span,
                            format!("duplicate pattern field `{}`", field_pat.kind.name),
                        );
                        continue;
                    }
                    let Some(field) = struct_item
                        .kind
                        .fields
                        .iter()
                        .find(|field| field.kind.name == field_pat.kind.name)
                    else {
                        self.error(
                            field_pat.span,
                            format!("unknown field `{}`", field_pat.kind.name),
                        );
                        continue;
                    };
                    let field_ty = self.apply_type_substitutions(&field.kind.ty, &substitutions);
                    let _ = self.check_pat(
                        &field_pat.kind.pat,
                        &field_ty,
                        scope,
                        locals,
                        seen_variants,
                    );
                }
                false
            }
        }
    }

    fn check_match_exhaustive(
        &mut self,
        span: Span,
        scrutinee_ty: &Ty,
        seen_variants: &HashSet<String>,
        has_wildcard: bool,
    ) {
        if has_wildcard {
            return;
        }
        let TyKind::Path { path, .. } = &scrutinee_ty.kind else {
            return;
        };
        let enum_name = path_name(path);
        let Some(enum_item) = self.enums.get(&enum_name) else {
            return;
        };
        for variant in &enum_item.kind.variants {
            if !seen_variants.contains(&variant.kind.name) {
                self.error(
                    span,
                    format!("match is not exhaustive: missing `{}`", variant.kind.name),
                );
                return;
            }
        }
    }

    fn generic_arg_substitutions(
        &self,
        generics: &[GenericParam],
        args: &[GenericArg],
    ) -> HashMap<String, Ty> {
        let mut substitutions = HashMap::new();
        for (generic, arg) in generics.iter().zip(args) {
            if let (GenericParamKind::Type { name, .. }, GenericArgKind::Ty(ty)) =
                (&generic.kind, &arg.kind)
            {
                substitutions.insert(name.clone(), ty.clone());
            }
        }
        substitutions
    }

    fn check_block(
        &mut self,
        block: &Block,
        scope: &GenericScope,
        locals: &mut HashMap<String, Ty>,
        expected_ret: &Ty,
    ) -> ExprTy {
        let mut block_locals = locals.clone();
        let mut diverges = false;
        for stmt in &block.kind.stmts {
            match &stmt.kind {
                StmtKind::Let(let_stmt) => {
                    let init_ty =
                        let_stmt.kind.init.as_ref().map(|init| {
                            self.check_expr(init, scope, &mut block_locals, expected_ret)
                        });
                    if let Some(ty) = &let_stmt.kind.ty {
                        self.check_ty_wf(ty, scope, false);
                        if let Some(init_ty) = &init_ty {
                            if !self.ty_eq(&init_ty.ty, ty, scope) {
                                self.error(
                                    let_stmt.span,
                                    format!(
                                        "let initializer has type `{}`, expected `{}`",
                                        self.ty_display(&init_ty.ty),
                                        self.ty_display(ty)
                                    ),
                                );
                            }
                        }
                        block_locals.insert(let_stmt.kind.name.clone(), ty.clone());
                    } else if let Some(init_ty) = init_ty {
                        block_locals.insert(let_stmt.kind.name.clone(), init_ty.ty);
                    } else {
                        self.error(let_stmt.span, "let statement needs a type or initializer");
                    }
                }
                StmtKind::Expr(expr) | StmtKind::Semi(expr) => {
                    let ty = self.check_expr(expr, scope, &mut block_locals, expected_ret);
                    diverges |= ty.diverges;
                }
            }
        }
        if let Some(tail) = &block.kind.tail {
            let tail_ty = self.check_expr(tail, scope, &mut block_locals, expected_ret);
            ExprTy {
                ty: tail_ty.ty,
                diverges: diverges || tail_ty.diverges,
            }
        } else {
            ExprTy {
                ty: Node::new(block.span, TyKind::Unit),
                diverges,
            }
        }
    }

    fn lookup_field_type(&self, ty: &Ty, name: &str) -> Option<Ty> {
        let TyKind::Path { path, args } = &ty.kind else {
            return None;
        };
        let item = self.structs.get(&path_name(path))?;
        let substitutions = self.generic_arg_substitutions(&item.kind.generics, args);
        item.kind
            .fields
            .iter()
            .find(|field| field.kind.name == name)
            .map(|field| self.apply_type_substitutions(&field.kind.ty, &substitutions))
    }

    fn check_ty_wf(&mut self, ty: &Ty, scope: &GenericScope, allow_self: bool) {
        match &ty.kind {
            TyKind::Unit | TyKind::Bool | TyKind::Never | TyKind::Int(_) => {}
            TyKind::SelfTy => {
                if !allow_self {
                    self.error(ty.span, "`Self` is not valid in this type position");
                }
            }
            TyKind::Path { path, args } => {
                let name = path_name(path);
                if scope.type_params.contains_key(&name) {
                    if !args.is_empty() {
                        self.error(ty.span, "type parameter cannot take generic arguments");
                    }
                    return;
                }
                if self.interfaces.contains_key(&name) {
                    self.error(
                        ty.span,
                        format!(
                            "interface `{name}` is not a value type; use a generic bound instead"
                        ),
                    );
                    return;
                }
                let generic_params = self
                    .structs
                    .get(&name)
                    .map(|item| item.kind.generics.clone())
                    .or_else(|| self.enums.get(&name).map(|item| item.kind.generics.clone()));
                match &generic_params {
                    Some(expected) if expected.len() == args.len() => {}
                    Some(expected) => self.error(
                        ty.span,
                        format!(
                            "type `{name}` expects {} generic arguments, got {}",
                            expected.len(),
                            args.len()
                        ),
                    ),
                    None => self.error(ty.span, format!("unknown type `{name}`")),
                }
                if let Some(generic_params) = generic_params {
                    for (arg, generic) in args.iter().zip(&generic_params) {
                        match (&generic.kind, &arg.kind) {
                            (GenericParamKind::Type { .. }, GenericArgKind::Ty(ty)) => {
                                self.check_ty_wf(ty, scope, false);
                            }
                            (GenericParamKind::Type { name, .. }, GenericArgKind::Const(_)) => {
                                self.error(
                                    arg.span,
                                    format!("generic parameter `{name}` expects a type argument"),
                                );
                            }
                            (GenericParamKind::Const { name, ty }, GenericArgKind::Const(expr)) => {
                                self.check_const_arg(expr, *ty, name, scope)
                            }
                            (GenericParamKind::Const { name, ty }, GenericArgKind::Ty(ty_arg)) => {
                                let Some(expr) = self.ty_arg_as_const_expr(ty_arg) else {
                                    self.error(
                                        arg.span,
                                        format!(
                                            "generic parameter `{name}` expects a const argument"
                                        ),
                                    );
                                    continue;
                                };
                                self.check_const_arg(&expr, *ty, name, scope);
                            }
                        }
                    }
                }
            }
            TyKind::Array { elem, len } => {
                self.check_ty_wf(elem, scope, false);
                if let Some(value) = self.eval_const_expr(len, scope) {
                    if value.sign() == Sign::Minus {
                        self.error(len.span, "array length must be nonnegative");
                    }
                }
            }
            TyKind::Ref { ty, .. } => self.check_ty_wf(ty, scope, allow_self),
            TyKind::Fn { params, ret } => {
                for param in params {
                    self.check_ty_wf(param, scope, allow_self);
                }
                self.check_ty_wf(ret, scope, allow_self);
            }
        }
    }

    fn check_const_arg(&mut self, expr: &ConstExpr, ty: IntTy, name: &str, scope: &GenericScope) {
        if let Some(value) = self.eval_const_expr(expr, scope) {
            if !fits_int_ty(&value, ty) {
                self.error(
                    expr.span,
                    format!(
                        "const argument for `{name}` has value `{value}` which does not fit in {ty}"
                    ),
                );
            }
        }
    }

    fn ty_arg_as_const_expr(&self, ty: &Ty) -> Option<ConstExpr> {
        let TyKind::Path { path, args } = &ty.kind else {
            return None;
        };
        if !args.is_empty() {
            return None;
        }
        Some(Node::new(ty.span, ConstExprKind::Path(path.clone())))
    }

    fn check_where_predicates(&mut self, predicates: &[WherePredicate], scope: &GenericScope) {
        for predicate in predicates {
            match &predicate.kind {
                WherePredicateKind::Implements { ty, interface } => {
                    self.check_ty_wf(ty, scope, false);
                    self.check_interface_ref(interface, scope);
                }
                WherePredicateKind::ConstEq { lhs, rhs } => {
                    let lhs_value = self.eval_const_expr(lhs, scope);
                    let rhs_value = self.eval_const_expr(rhs, scope);
                    if let (Some(lhs_value), Some(rhs_value)) = (lhs_value, rhs_value) {
                        if lhs_value != rhs_value {
                            self.error(
                                predicate.span,
                                format!(
                                    "const equality predicate failed: `{lhs_value}` != `{rhs_value}`"
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    fn check_interface_ref(&mut self, interface: &InterfaceRef, scope: &GenericScope) {
        let name = path_name(&interface.kind.path);
        let expected = self
            .interfaces
            .get(&name)
            .map(|item| item.kind.generics.len());
        match expected {
            Some(expected) if expected == interface.kind.args.len() => {}
            Some(expected) => self.error(
                interface.span,
                format!(
                    "interface `{name}` expects {expected} generic arguments, got {}",
                    interface.kind.args.len()
                ),
            ),
            None => self.error(interface.span, format!("unknown interface `{name}`")),
        }
        for arg in &interface.kind.args {
            match &arg.kind {
                GenericArgKind::Ty(ty) => self.check_ty_wf(ty, scope, false),
                GenericArgKind::Const(expr) => {
                    self.eval_const_expr(expr, scope);
                }
            }
        }
    }

    fn eval_const_expr(&mut self, expr: &ConstExpr, scope: &GenericScope) -> Option<BigInt> {
        match &expr.kind {
            ConstExprKind::IntLit(value) => Some(value.clone()),
            ConstExprKind::Path(path) => {
                let name = path_name(path);
                if scope.const_params.contains_key(&name) {
                    return None;
                }
                if self.const_items.contains_key(&name) {
                    return self.eval_top_const(&name).map(|value| value.value);
                }
                self.error(expr.span, format!("unknown const `{name}`"));
                None
            }
            ConstExprKind::Param(name) => {
                if !scope.const_params.contains_key(name) {
                    self.error(expr.span, format!("unknown const parameter `{name}`"));
                }
                None
            }
            ConstExprKind::AssocConst {
                ty,
                interface,
                name,
            } => {
                let interface_ref = Node::new(
                    interface.span,
                    InterfaceRefKind {
                        path: interface.clone(),
                        args: vec![],
                    },
                );
                if !self.proves_implements(ty, &interface_ref, scope) {
                    self.error(
                        expr.span,
                        format!(
                            "cannot prove `{}` implements `{}` for associated const `{name}`",
                            self.ty_display(ty),
                            path_name(interface)
                        ),
                    );
                }
                self.resolve_assoc_const_value(ty, interface, name, scope)
            }
            ConstExprKind::Unary { op, expr: inner } => {
                let value = self.eval_const_expr(inner, scope)?;
                match op {
                    ConstUnaryOp::Plus => Some(value),
                    ConstUnaryOp::Neg => Some(-value),
                }
            }
            ConstExprKind::Binary { op, lhs, rhs } => {
                let lhs = self.eval_const_expr(lhs, scope)?;
                let rhs = self.eval_const_expr(rhs, scope)?;
                match op {
                    ConstBinaryOp::Add => Some(lhs + rhs),
                    ConstBinaryOp::Sub => Some(lhs - rhs),
                    ConstBinaryOp::Mul => Some(lhs * rhs),
                    ConstBinaryOp::Div => {
                        if rhs.is_zero() {
                            self.error(expr.span, "division by zero in const expression");
                            None
                        } else {
                            Some(lhs / rhs)
                        }
                    }
                    ConstBinaryOp::Rem => {
                        if rhs.is_zero() {
                            self.error(expr.span, "remainder by zero in const expression");
                            None
                        } else {
                            Some(lhs % rhs)
                        }
                    }
                    ConstBinaryOp::Shl => rhs.to_usize().map(|shift| lhs << shift).or_else(|| {
                        self.error(expr.span, "shift amount does not fit usize");
                        None
                    }),
                    ConstBinaryOp::Shr => rhs.to_usize().map(|shift| lhs >> shift).or_else(|| {
                        self.error(expr.span, "shift amount does not fit usize");
                        None
                    }),
                    ConstBinaryOp::BitAnd => Some(lhs & rhs),
                    ConstBinaryOp::BitOr => Some(lhs | rhs),
                    ConstBinaryOp::BitXor => Some(lhs ^ rhs),
                }
            }
            ConstExprKind::Cast { expr: inner, ty } => {
                let value = self.eval_const_expr(inner, scope)?;
                if !fits_int_ty(&value, *ty) {
                    self.error(expr.span, format!("value `{value}` does not fit in {ty}"));
                    None
                } else {
                    Some(value)
                }
            }
        }
    }

    fn method_sigs_match(
        &mut self,
        required: &MethodSig,
        provided: &MethodSig,
        self_ty: &Ty,
        scope: &GenericScope,
    ) -> bool {
        if required.kind.receiver.as_ref().map(|r| &r.kind)
            != provided.kind.receiver.as_ref().map(|r| &r.kind)
        {
            return false;
        }
        if required.kind.params.len() != provided.kind.params.len() {
            return false;
        }
        for (a, b) in required.kind.params.iter().zip(&provided.kind.params) {
            let a = self.substitute_self(&a.kind.ty, self_ty);
            let b = self.substitute_self(&b.kind.ty, self_ty);
            if !self.ty_eq(&a, &b, scope) {
                return false;
            }
        }
        let a = self.substitute_self(&required.kind.ret, self_ty);
        let b = self.substitute_self(&provided.kind.ret, self_ty);
        self.ty_eq(&a, &b, scope)
    }

    fn method_sigs_equivalent(&self, a: &MethodSig, b: &MethodSig, scope: &GenericScope) -> bool {
        a.kind.receiver.as_ref().map(|r| &r.kind) == b.kind.receiver.as_ref().map(|r| &r.kind)
            && self.generic_params_equivalent(&a.kind.generics, &b.kind.generics, scope)
            && a.kind.params.len() == b.kind.params.len()
            && a.kind
                .params
                .iter()
                .zip(&b.kind.params)
                .all(|(a, b)| self.ty_eq_no_diag(&a.kind.ty, &b.kind.ty, scope))
            && self.ty_eq_no_diag(&a.kind.ret, &b.kind.ret, scope)
    }

    fn generic_params_equivalent(
        &self,
        a: &[GenericParam],
        b: &[GenericParam],
        scope: &GenericScope,
    ) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| match (&a.kind, &b.kind) {
                (
                    GenericParamKind::Type {
                        name: an,
                        bounds: ab,
                    },
                    GenericParamKind::Type {
                        name: bn,
                        bounds: bb,
                    },
                ) => {
                    an == bn
                        && ab.len() == bb.len()
                        && ab
                            .iter()
                            .zip(bb)
                            .all(|(a, b)| self.interface_refs_equivalent(a, b, scope))
                }
                (
                    GenericParamKind::Const { name: an, ty: at },
                    GenericParamKind::Const { name: bn, ty: bt },
                ) => an == bn && at == bt,
                _ => false,
            })
    }

    fn interface_refs_equivalent(
        &self,
        a: &InterfaceRef,
        b: &InterfaceRef,
        scope: &GenericScope,
    ) -> bool {
        path_name(&a.kind.path) == path_name(&b.kind.path)
            && a.kind.args.len() == b.kind.args.len()
            && a.kind
                .args
                .iter()
                .zip(&b.kind.args)
                .all(|(a, b)| match (&a.kind, &b.kind) {
                    (GenericArgKind::Ty(a), GenericArgKind::Ty(b)) => {
                        self.ty_eq_no_diag(a, b, scope)
                    }
                    (GenericArgKind::Const(a), GenericArgKind::Const(b)) => {
                        self.const_eq_no_diag(a, b, scope)
                    }
                    _ => false,
                })
    }

    fn const_eq_no_diag(&self, a: &ConstExpr, b: &ConstExpr, scope: &GenericScope) -> bool {
        let mut checker = self.checker_for_compare();
        checker.const_eq(a, b, scope, false)
    }

    fn interface_reqs(&self, interface: &InterfaceRef) -> Option<InterfaceReqs> {
        let mut stack = vec![];
        self.interface_reqs_no_diag(interface, &mut stack)
    }

    fn interface_reqs_no_diag(
        &self,
        interface: &InterfaceRef,
        stack: &mut Vec<String>,
    ) -> Option<InterfaceReqs> {
        let name = path_name(&interface.kind.path);
        if stack.contains(&name) {
            return None;
        }
        let item = self.interfaces.get(&name).copied()?;
        stack.push(name);
        let mut reqs = InterfaceReqs::default();
        for super_interface in &item.kind.super_interfaces {
            if let Some(parent) = self.interface_reqs_no_diag(super_interface, stack) {
                reqs.methods.extend(parent.methods);
                reqs.consts.extend(parent.consts);
            }
        }
        for member in &item.kind.members {
            match &member.kind {
                InterfaceMemberKind::Method(sig) => {
                    reqs.methods.insert(sig.kind.name.clone(), sig.clone());
                }
                InterfaceMemberKind::AssocConst(sig) => {
                    reqs.consts.insert(sig.kind.name.clone(), sig.clone());
                }
            }
        }
        stack.pop();
        Some(reqs)
    }

    fn interface_reqs_checked(
        &mut self,
        interface: &InterfaceRef,
        stack: &mut Vec<String>,
        scope: &GenericScope,
    ) -> Option<InterfaceReqs> {
        let name = path_name(&interface.kind.path);
        if stack.contains(&name) {
            self.error(
                interface.span,
                format!("interface inheritance cycle involving `{name}`"),
            );
            return None;
        }
        let Some(item) = self.interfaces.get(&name).copied() else {
            return None;
        };
        stack.push(name);
        let mut reqs = InterfaceReqs::default();
        for super_interface in &item.kind.super_interfaces {
            if let Some(parent) = self.interface_reqs_checked(super_interface, stack, scope) {
                self.merge_interface_reqs(&mut reqs, parent, super_interface.span, scope);
            }
        }
        for member in &item.kind.members {
            match &member.kind {
                InterfaceMemberKind::Method(sig) => {
                    self.insert_method_req(&mut reqs, sig.clone(), sig.span, scope);
                }
                InterfaceMemberKind::AssocConst(sig) => {
                    self.insert_const_req(&mut reqs, sig.clone(), sig.span);
                }
            }
        }
        stack.pop();
        Some(reqs)
    }

    fn merge_interface_reqs(
        &mut self,
        dst: &mut InterfaceReqs,
        src: InterfaceReqs,
        span: Span,
        scope: &GenericScope,
    ) {
        for (_, sig) in src.methods {
            self.insert_method_req(dst, sig, span, scope);
        }
        for (_, sig) in src.consts {
            self.insert_const_req(dst, sig, span);
        }
    }

    fn insert_method_req(
        &mut self,
        reqs: &mut InterfaceReqs,
        sig: MethodSig,
        span: Span,
        scope: &GenericScope,
    ) {
        if let Some(existing) = reqs.methods.get(&sig.kind.name) {
            if !self.method_sigs_equivalent(existing, &sig, scope) {
                self.error(
                    span,
                    format!("conflicting inherited method `{}`", sig.kind.name),
                );
            }
            return;
        }
        reqs.methods.insert(sig.kind.name.clone(), sig);
    }

    fn insert_const_req(&mut self, reqs: &mut InterfaceReqs, sig: AssocConstSig, span: Span) {
        if let Some(existing) = reqs.consts.get(&sig.kind.name) {
            if existing.kind.ty != sig.kind.ty {
                self.error(
                    span,
                    format!("conflicting inherited associated const `{}`", sig.kind.name),
                );
            }
            return;
        }
        reqs.consts.insert(sig.kind.name.clone(), sig);
    }

    fn current_interface_ref(&self, item: &InterfaceItem) -> InterfaceRef {
        let args = item
            .kind
            .generics
            .iter()
            .map(|generic| match &generic.kind {
                GenericParamKind::Type { name, .. } => {
                    let ty = Node::new(
                        generic.span,
                        TyKind::Path {
                            path: synthetic_path(name),
                            args: vec![],
                        },
                    );
                    Node::new(generic.span, GenericArgKind::Ty(ty))
                }
                GenericParamKind::Const { name, .. } => Node::new(
                    generic.span,
                    GenericArgKind::Const(Node::new(
                        generic.span,
                        ConstExprKind::Param(name.clone()),
                    )),
                ),
            })
            .collect();
        Node::new(
            item.span,
            InterfaceRefKind {
                path: synthetic_path(&item.kind.name),
                args,
            },
        )
    }

    fn resolve_assoc_const_value(
        &mut self,
        ty: &Ty,
        interface: &Path,
        name: &str,
        scope: &GenericScope,
    ) -> Option<BigInt> {
        let interface_ref = Node::new(
            interface.span,
            InterfaceRefKind {
                path: interface.clone(),
                args: vec![],
            },
        );
        for imp in self.impls.clone() {
            let Some(impl_interface) = &imp.kind.interface else {
                continue;
            };
            if !self.interface_implies(impl_interface, &interface_ref) {
                continue;
            }
            if !self.ty_eq_no_diag(ty, &imp.kind.self_ty, scope) {
                continue;
            }
            let impl_scope =
                self.scope_from_generics(&imp.kind.generics, &imp.kind.where_predicates);
            for member in &imp.kind.members {
                if let ImplMemberKind::AssocConst(assoc) = &member.kind {
                    if assoc.kind.name == name {
                        return self.eval_const_expr(&assoc.kind.expr, &impl_scope);
                    }
                }
            }
            if let Some(reqs) = self.interface_reqs(impl_interface) {
                if let Some(default) = reqs
                    .consts
                    .get(name)
                    .and_then(|sig| sig.kind.default.as_ref())
                {
                    return self.eval_const_expr(default, &impl_scope);
                }
            }
        }
        None
    }

    fn scope_from_generics(
        &self,
        generics: &[GenericParam],
        predicates: &[WherePredicate],
    ) -> GenericScope {
        let mut scope = GenericScope::default();
        self.extend_scope_with_generics(&mut scope, generics);
        for generic in generics {
            if let GenericParamKind::Type { name, bounds } = &generic.kind {
                let ty = Node::new(
                    generic.span,
                    TyKind::Path {
                        path: synthetic_path(name),
                        args: vec![],
                    },
                );
                for bound in bounds {
                    scope.obligations.push((ty.clone(), bound.clone()));
                }
            }
        }
        for predicate in predicates {
            if let WherePredicateKind::Implements { ty, interface } = &predicate.kind {
                scope.obligations.push((ty.clone(), interface.clone()));
            }
        }
        scope
    }

    fn extend_scope_with_generics(&self, scope: &mut GenericScope, generics: &[GenericParam]) {
        for generic in generics {
            match &generic.kind {
                GenericParamKind::Type { name, bounds } => {
                    scope.type_params.insert(name.clone(), bounds.clone());
                }
                GenericParamKind::Const { name, ty } => {
                    scope.const_params.insert(name.clone(), *ty);
                }
            }
        }
    }

    fn check_generic_param_names(&mut self, generics: &[GenericParam]) {
        let mut names = HashSet::new();
        for generic in generics {
            let name = match &generic.kind {
                GenericParamKind::Type { name, .. } | GenericParamKind::Const { name, .. } => name,
            };
            if !names.insert(name.clone()) {
                self.error(
                    generic.span,
                    format!("duplicate generic parameter `{name}`"),
                );
            }
        }
    }

    fn check_duplicate_fields(&mut self, fields: &[Field]) {
        let mut names = HashSet::new();
        for field in fields {
            if !names.insert(field.kind.name.clone()) {
                self.error(field.span, format!("duplicate field `{}`", field.kind.name));
            }
        }
    }

    fn proves_implements(&self, ty: &Ty, interface: &InterfaceRef, scope: &GenericScope) -> bool {
        scope.obligations.iter().any(|(ob_ty, ob_interface)| {
            self.ty_eq_no_diag(ty, ob_ty, scope) && self.interface_implies(ob_interface, interface)
        }) || self.impls.iter().any(|imp| {
            imp.kind
                .interface
                .as_ref()
                .map(|imp_interface| {
                    self.ty_eq_no_diag(ty, &imp.kind.self_ty, scope)
                        && self.interface_implies(imp_interface, interface)
                })
                .unwrap_or(false)
        })
    }

    fn interface_implies(&self, actual: &InterfaceRef, required: &InterfaceRef) -> bool {
        let required_name = path_name(&required.kind.path);
        let mut stack = vec![];
        self.interface_implies_name(actual, &required_name, &mut stack)
    }

    fn interface_implies_name(
        &self,
        actual: &InterfaceRef,
        required_name: &str,
        stack: &mut Vec<String>,
    ) -> bool {
        let actual_name = path_name(&actual.kind.path);
        if actual_name == required_name {
            return true;
        }
        if stack.contains(&actual_name) {
            return false;
        }
        let Some(item) = self.interfaces.get(&actual_name).copied() else {
            return false;
        };
        stack.push(actual_name);
        let result = item
            .kind
            .super_interfaces
            .iter()
            .any(|parent| self.interface_implies_name(parent, required_name, stack));
        stack.pop();
        result
    }

    fn substitute_self(&self, ty: &Ty, self_ty: &Ty) -> Ty {
        let _ = self.structs.len();
        match &ty.kind {
            TyKind::SelfTy => self_ty.clone(),
            TyKind::Array { elem, len } => Node::new(
                ty.span,
                TyKind::Array {
                    elem: Box::new(self.substitute_self(elem, self_ty)),
                    len: self.substitute_self_const(len, self_ty),
                },
            ),
            TyKind::Ref {
                mutability,
                ty: inner,
            } => Node::new(
                ty.span,
                TyKind::Ref {
                    mutability: *mutability,
                    ty: Box::new(self.substitute_self(inner, self_ty)),
                },
            ),
            TyKind::Fn { params, ret } => Node::new(
                ty.span,
                TyKind::Fn {
                    params: params
                        .iter()
                        .map(|p| self.substitute_self(p, self_ty))
                        .collect(),
                    ret: Box::new(self.substitute_self(ret, self_ty)),
                },
            ),
            TyKind::Path { path, args } => Node::new(
                ty.span,
                TyKind::Path {
                    path: path.clone(),
                    args: args
                        .iter()
                        .map(|arg| match &arg.kind {
                            GenericArgKind::Ty(ty) => Node::new(
                                arg.span,
                                GenericArgKind::Ty(self.substitute_self(ty, self_ty)),
                            ),
                            GenericArgKind::Const(expr) => Node::new(
                                arg.span,
                                GenericArgKind::Const(self.substitute_self_const(expr, self_ty)),
                            ),
                        })
                        .collect(),
                },
            ),
            _ => ty.clone(),
        }
    }

    fn substitute_self_const(&self, expr: &ConstExpr, self_ty: &Ty) -> ConstExpr {
        match &expr.kind {
            ConstExprKind::AssocConst {
                ty,
                interface,
                name,
            } => Node::new(
                expr.span,
                ConstExprKind::AssocConst {
                    ty: Box::new(self.substitute_self(ty, self_ty)),
                    interface: interface.clone(),
                    name: name.clone(),
                },
            ),
            ConstExprKind::Unary { op, expr: inner } => Node::new(
                expr.span,
                ConstExprKind::Unary {
                    op: *op,
                    expr: Box::new(self.substitute_self_const(inner, self_ty)),
                },
            ),
            ConstExprKind::Binary { op, lhs, rhs } => Node::new(
                expr.span,
                ConstExprKind::Binary {
                    op: *op,
                    lhs: Box::new(self.substitute_self_const(lhs, self_ty)),
                    rhs: Box::new(self.substitute_self_const(rhs, self_ty)),
                },
            ),
            ConstExprKind::Cast { expr: inner, ty } => Node::new(
                expr.span,
                ConstExprKind::Cast {
                    expr: Box::new(self.substitute_self_const(inner, self_ty)),
                    ty: *ty,
                },
            ),
            _ => expr.clone(),
        }
    }

    fn apply_type_substitutions(&self, ty: &Ty, substitutions: &HashMap<String, Ty>) -> Ty {
        match &ty.kind {
            TyKind::Path { path, args } if path.kind.segments.len() == 1 && args.is_empty() => {
                substitutions
                    .get(&path.kind.segments[0])
                    .cloned()
                    .unwrap_or_else(|| ty.clone())
            }
            TyKind::Path { path, args } => Node::new(
                ty.span,
                TyKind::Path {
                    path: path.clone(),
                    args: args
                        .iter()
                        .map(|arg| match &arg.kind {
                            GenericArgKind::Ty(ty) => Node::new(
                                arg.span,
                                GenericArgKind::Ty(
                                    self.apply_type_substitutions(ty, substitutions),
                                ),
                            ),
                            GenericArgKind::Const(expr) => {
                                Node::new(arg.span, GenericArgKind::Const(expr.clone()))
                            }
                        })
                        .collect(),
                },
            ),
            TyKind::Array { elem, len } => Node::new(
                ty.span,
                TyKind::Array {
                    elem: Box::new(self.apply_type_substitutions(elem, substitutions)),
                    len: len.clone(),
                },
            ),
            TyKind::Ref {
                mutability,
                ty: inner,
            } => Node::new(
                ty.span,
                TyKind::Ref {
                    mutability: *mutability,
                    ty: Box::new(self.apply_type_substitutions(inner, substitutions)),
                },
            ),
            _ => ty.clone(),
        }
    }

    fn ty_eq(&mut self, a: &Ty, b: &Ty, scope: &GenericScope) -> bool {
        self.ty_eq_impl(a, b, scope, true)
    }

    fn ty_eq_no_diag(&self, a: &Ty, b: &Ty, scope: &GenericScope) -> bool {
        let mut checker = self.checker_for_compare();
        checker.ty_eq_impl(a, b, scope, false)
    }

    fn checker_for_compare(&self) -> Checker<'a> {
        Checker {
            program: self.program,
            structs: self.structs.clone(),
            enums: self.enums.clone(),
            interfaces: self.interfaces.clone(),
            impls: self.impls.clone(),
            fns: self.fns.clone(),
            const_items: self.const_items.clone(),
            const_values: self.const_values.clone(),
            evaluating_consts: HashSet::new(),
            diagnostics: vec![],
        }
    }

    fn ty_eq_impl(&mut self, a: &Ty, b: &Ty, scope: &GenericScope, emit_errors: bool) -> bool {
        match (&a.kind, &b.kind) {
            (TyKind::Unit, TyKind::Unit)
            | (TyKind::Bool, TyKind::Bool)
            | (TyKind::Never, TyKind::Never)
            | (TyKind::SelfTy, TyKind::SelfTy) => true,
            (TyKind::Int(a), TyKind::Int(b)) => a == b,
            (TyKind::Path { path: ap, args: aa }, TyKind::Path { path: bp, args: ba }) => {
                path_name(ap) == path_name(bp)
                    && aa.len() == ba.len()
                    && aa.iter().zip(ba).all(|(a, b)| match (&a.kind, &b.kind) {
                        (GenericArgKind::Ty(a), GenericArgKind::Ty(b)) => {
                            self.ty_eq_impl(a, b, scope, emit_errors)
                        }
                        (GenericArgKind::Const(a), GenericArgKind::Const(b)) => {
                            self.const_eq(a, b, scope, emit_errors)
                        }
                        _ => false,
                    })
            }
            (TyKind::Array { elem: ae, len: al }, TyKind::Array { elem: be, len: bl }) => {
                self.ty_eq_impl(ae, be, scope, emit_errors)
                    && self.const_eq(al, bl, scope, emit_errors)
            }
            (
                TyKind::Ref {
                    mutability: am,
                    ty: at,
                },
                TyKind::Ref {
                    mutability: bm,
                    ty: bt,
                },
            ) => am == bm && self.ty_eq_impl(at, bt, scope, emit_errors),
            (
                TyKind::Fn {
                    params: ap,
                    ret: ar,
                },
                TyKind::Fn {
                    params: bp,
                    ret: br,
                },
            ) => {
                ap.len() == bp.len()
                    && ap
                        .iter()
                        .zip(bp)
                        .all(|(a, b)| self.ty_eq_impl(a, b, scope, emit_errors))
                    && self.ty_eq_impl(ar, br, scope, emit_errors)
            }
            (TyKind::Never, _) | (_, TyKind::Never) => true,
            _ => false,
        }
    }

    fn const_eq(
        &mut self,
        a: &ConstExpr,
        b: &ConstExpr,
        scope: &GenericScope,
        emit_errors: bool,
    ) -> bool {
        let av = self.eval_const_expr(a, scope);
        let bv = self.eval_const_expr(b, scope);
        match (av, bv) {
            (Some(a), Some(b)) => a == b,
            (None, None) => a.kind == b.kind,
            _ => {
                if emit_errors {
                    self.error(a.span.join(b.span), "const expressions are not equal");
                }
                false
            }
        }
    }

    fn ty_display(&self, ty: &Ty) -> String {
        match &ty.kind {
            TyKind::Unit => "()".to_owned(),
            TyKind::Bool => "bool".to_owned(),
            TyKind::Never => "!".to_owned(),
            TyKind::Int(ty) => ty.to_string(),
            TyKind::SelfTy => "Self".to_owned(),
            TyKind::Path { path, args } => {
                if args.is_empty() {
                    path_name(path)
                } else {
                    let args = args
                        .iter()
                        .map(|arg| match &arg.kind {
                            GenericArgKind::Ty(ty) => self.ty_display(ty),
                            GenericArgKind::Const(expr) => self.const_display(expr),
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}<{args}>", path_name(path))
                }
            }
            TyKind::Array { elem, len } => {
                format!("[{}; {}]", self.ty_display(elem), self.const_display(len))
            }
            TyKind::Ref { mutability, ty } => match mutability {
                Mutability::Shared => format!("&{}", self.ty_display(ty)),
                Mutability::Mutable => format!("&mut {}", self.ty_display(ty)),
            },
            TyKind::Fn { params, ret } => {
                let params = params
                    .iter()
                    .map(|p| self.ty_display(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({params}) -> {}", self.ty_display(ret))
            }
        }
    }

    fn const_display(&self, expr: &ConstExpr) -> String {
        let _ = self.structs.len();
        match &expr.kind {
            ConstExprKind::IntLit(value) => value.to_string(),
            ConstExprKind::Path(path) => path_name(path),
            ConstExprKind::Param(name) => name.clone(),
            ConstExprKind::AssocConst {
                ty,
                interface,
                name,
            } => {
                format!(
                    "<{} as {}>::{}",
                    self.ty_display(ty),
                    path_name(interface),
                    name
                )
            }
            ConstExprKind::Unary { op, expr } => match op {
                ConstUnaryOp::Plus => format!("+{}", self.const_display(expr)),
                ConstUnaryOp::Neg => format!("-{}", self.const_display(expr)),
            },
            ConstExprKind::Binary { op, lhs, rhs } => {
                format!(
                    "({} {:?} {})",
                    self.const_display(lhs),
                    op,
                    self.const_display(rhs)
                )
            }
            ConstExprKind::Cast { expr, ty } => format!("{} as {}", self.const_display(expr), ty),
        }
    }

    fn error_ty(&self, span: Span) -> ExprTy {
        ExprTy {
            ty: Node::new(span, TyKind::Never),
            diverges: true,
        }
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }
}

fn peel_ref(ty: &Ty) -> Option<&Ty> {
    match &ty.kind {
        TyKind::Ref { ty, .. } => Some(ty),
        _ => None,
    }
}

fn fits_int_ty(value: &BigInt, ty: IntTy) -> bool {
    let bits = ty.bits();
    match ty.signedness {
        Signedness::Unsigned => {
            if value.sign() == Sign::Minus {
                return false;
            }
            let max = (BigInt::one() << bits) - BigInt::one();
            value <= &max
        }
        Signedness::Signed => {
            let min = -(BigInt::one() << (bits - 1));
            let max = (BigInt::one() << (bits - 1)) - BigInt::one();
            value >= &min && value <= &max
        }
    }
}
