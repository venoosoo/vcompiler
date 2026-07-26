use std::{collections::HashMap, env, fs::File};

use super::*;

use crate::{
    Ir::{
        Stmt,
        expr::Expr,
        r#gen::{StructData, VarData},
        sem_analysis::{Analyzer, SemanticError},
        stmt::{Declaration, LValue, MatchField, MatchLeftValue, StructDef, Type},
    },
    shared::lvalue_root,
    tokenizer::TokenType,
};

impl<'a> Analyzer<'a> {
    pub fn check_block(&mut self, data: &Vec<Stmt>) {
        self.scopes.push(HashMap::new());
        for i in data.iter() {
            self.check_stmt(i);
        }
        self.scopes.pop();
    }

    pub fn check_declaration(&mut self, data: &Declaration) {
        let ty = self.ensure_monomorphized(&data.ty);
        if ty == Type::Primitive(TokenType::Void) {
            self.print_error(self.type_to_error(SemanticError::VoidVariable(data.name.clone())));
            return;
        }
        if self.lookup(&data.name).is_some() {
            self.print_error(self.type_to_error(SemanticError::AlreadyDeclared(data.name.clone())));
        }

        if let Some(expr) = &data.initializer {
            let expr_ty = self.check_expr(expr, &ty);
            let expr_ty = self.ensure_monomorphized(&expr_ty);
            if !check_types(&ty, &expr_ty) {
                self.print_error(self.type_to_error(SemanticError::TypeMismatch {
                    expected: ty.clone(),
                    got: expr_ty.clone(),
                }));
            }
            if let (Type::Array(_, decl_size), Type::Array(_, init_size)) = (&ty, &expr_ty) {
                if init_size > decl_size {
                    self.print_error(self.type_to_error(SemanticError::ArrayTooLarge {
                        arr_name: data.name.clone(),
                        expected: *decl_size,
                        got: *init_size,
                    }));
                }
            }
        }
        self.add_var(data.name.clone(), data.ty.clone());
    }

    fn field_ty_match(&mut self, ty: &Type, name: &String) -> Type {
        let ty = self.ensure_monomorphized(ty);
        match &ty {
            Type::Pointer(p_ty) => {
                let res = self.field_ty_match(p_ty, name);
                res
            }

            Type::Struct(struct_name) => self
                .structs
                .get(struct_name)
                .and_then(|s| s.elements.get(name))
                .map(|f| f.ty.clone())
                .unwrap_or(Type::Unknown),
            _ => Type::Unknown,
        }
    }

    pub fn lvalue_type(&mut self, lvalue: &LValue) -> Type {
        match lvalue {
            LValue::Variable(name) => self.lookup(name).unwrap_or(Type::Unknown),
            LValue::Deref(inner) => {
                let inner_ty = self.lvalue_type(inner);
                match inner_ty {
                    Type::Pointer(ty) => *ty,
                    _ => Type::Unknown,
                }
            }
            LValue::Field { base, name } => {
                let base_ty = self.lvalue_type(base);
                self.field_ty_match(&base_ty, name)
            }
            LValue::Index { base, .. } => {
                let base_ty = self.lvalue_type(base);
                match base_ty {
                    Type::Array(elem_ty, _) => *elem_ty,
                    Type::Pointer(elem_ty) => *elem_ty,
                    _ => Type::Unknown,
                }
            }
        }
    }

    pub fn check_assignment(&mut self, target: &LValue, value: &Expr) {
        let var_name = lvalue_root(target);
        if self.lookup(&var_name).is_none() {
            self.print_error(self.type_to_error(SemanticError::UndeclaredVariable(var_name)));
            return;
        }

        // use lvalue_type for actual type check
        let target_ty = self.lvalue_type(target);
        let expr_ty = self.check_expr(value, &target_ty);
        if !check_types(&target_ty, &expr_ty) {
            self.print_error(self.type_to_error(SemanticError::TypeMismatch {
                expected: target_ty,
                got: expr_ty,
            }));
        }
    }

    pub fn check_if(
        &mut self,
        condition: &Expr,
        if_block: &Box<Stmt>,
        else_block: &Option<Box<Stmt>>,
    ) {
        let _expr_ty = self.check_expr(condition, &Type::Primitive(TokenType::I64));
        self.check_stmt(if_block);
        if let Some(else_data) = &else_block {
            self.check_stmt(else_data);
        }
    }

    pub fn check_while(&mut self, condition: &Expr, body: &Box<Stmt>) {
        let _expr_ty = self.check_expr(condition, &Type::Primitive(TokenType::I64));
        self.break_stack.push("1".to_string());
        self.contniue_stack.push("1".to_string());
        self.check_stmt(body);
        self.break_stack.pop();
        self.contniue_stack.pop();
    }

    pub fn check_for(
        &mut self,
        data: (
            &Option<Box<Stmt>>,
            &Option<Expr>,
            &Option<Box<Stmt>>,
            &Box<Stmt>,
        ),
    ) {
        let (init, condition, update, body) = data;
        self.scopes.push(HashMap::new());
        if let Some(init_data) = init {
            self.check_stmt(init_data);
        }
        if let Some(condition_data) = condition {
            self.check_expr(condition_data, &Type::Primitive(TokenType::I64));
        }
        if let Some(update_data) = update {
            self.check_stmt(update_data);
        }
        // doesnt matter what we push it wouldnt be used anyway
        // we only want to check if the break or continue inside loop
        self.break_stack.push("1".to_string());
        self.contniue_stack.push("1".to_string());
        self.check_stmt(body);
        self.break_stack.pop();
        self.contniue_stack.pop();
        self.scopes.pop();
    }

    pub fn check_ret(&mut self, expr: &Option<Expr>) {
        let mut expr_ty = Type::Primitive(TokenType::Void);
        if let Some(expr) = expr {
            expr_ty = self.check_expr(expr, &self.current_ret_type.clone());

            expr_ty = self.ensure_monomorphized(&expr_ty);
        }
        if !check_types(&self.current_ret_type, &expr_ty) {
            self.print_error(self.type_to_error(SemanticError::ReturnTypeMismatch {
                expected: self.current_ret_type.clone(),
                got: expr_ty.clone(),
            }));
        }
    }

    pub fn check_init_func(
        &mut self,
        data: (
            &String,
            &Vec<Declaration>,
            &Type,
            &Box<Stmt>,
            &HashMap<String, Type>,
        ),
    ) {
        let (name, args, ret_type, body, generic_types) = data;

        if self.functions.get(name).is_none() {
            println!("something strange inside check_init_func");
        }

        // save outer scopes FIRST before adding any args
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);

        self.scopes.push(HashMap::new());

        for arg in args.iter() {
            let ty = self.ensure_monomorphized(&arg.ty);
            let map = self.scopes.last_mut().unwrap();
            map.insert(arg.name.clone(), ty);
        }
        self.current_ret_type = self.ensure_monomorphized(&ret_type.clone());
        self.check_stmt(body);

        // restore outer scopes
        self.scopes = saved_scopes;
    }

    pub fn check_extern_fn(&mut self, data: &Box<Stmt>) {
        match &data.ty {
            StmtType::InitFunc {
                name,
                generic_types,
                args,
                ret_type,
                data,
            } => {
                self.check_init_func((name, args, ret_type, data, generic_types));
            }
            _ => unreachable!(),
        }
    }

    pub fn check_struct_init(&mut self, data: &StructDef) {
        if self.lookup(&data.name).is_some() {
            self.print_error(self.type_to_error(SemanticError::AlreadyDeclared(data.name.clone())));
        } else {
            let mut elements = IndexMap::new();
            for field in &data.fields {
                elements.insert(field.name.clone(), field.clone());
            }

            let size = self.compute_struct_size(&data.fields);

            let struct_data = StructData {
                name: data.name.clone(),
                generic_type: Vec::new(),
                elements,
                size,
            };
            self.structs.insert(data.name.clone(), struct_data);
        }
    }

    fn get_match_left_value_type(&self, lvalue: &MatchLeftValue) -> Type {
        match lvalue {
            MatchLeftValue::Enum { base, value, args } => {
                return Type::Enum(base.clone(), None);
            }
            MatchLeftValue::Expr { expr } => expr.get_type(self),
        }
    }

    fn check_match(&mut self, expr: &Expr, variants: &Vec<MatchField>) {
        let expr_ty = expr.get_type(self);
        let expr_ty = self.ensure_monomorphized(&expr_ty);

        match expr_ty.clone() {
            Type::Enum(_, _) | Type::Primitive(_) => {
                for var in variants {
                    let left_ty = self.get_match_left_value_type(&var.left);

                    if !check_types(&left_ty, &expr_ty) {
                        if let Type::Enum(name, _) = &left_ty {
                            if name == "_" {
                                continue;
                            }
                        }

                        if is_number(&left_ty) && is_number(&expr_ty) {
                            continue;
                        }

                        self.print_error(self.type_to_error(SemanticError::MatchTypeMismatch {
                            expected: expr_ty.clone(),
                            got: left_ty.clone(),
                        }));
                    }
                }
            }

            _ => {
                self.print_error(
                    self.type_to_error(SemanticError::MatchExprUnsuported(expr_ty.clone())),
                );
            }
        }
    }

    fn check_break(&self) {
        if self.break_stack.is_empty() {
            self.type_to_error(SemanticError::BreakOutsideOfLoop);
        }
    }

    fn check_contniue(&self) {
        if self.break_stack.is_empty() {
            self.type_to_error(SemanticError::BreakOutsideOfLoop);
        }
    }

    pub fn check_stmt(&mut self, stmt: &Stmt) {
        self.line = stmt.line;
        self.current_file = stmt.file.clone();
        match &stmt.ty {
            StmtType::Break => self.check_break(),
            StmtType::Continue => self.check_contniue(),
            StmtType::Block(data) => self.check_block(data),
            StmtType::Declaration(data) => self.check_declaration(data),
            StmtType::Assignment { target, value } => self.check_assignment(target, value),
            StmtType::ExprStmt(expr) => {
                self.check_expr(expr, &Type::Primitive(TokenType::I64));
            }
            StmtType::If {
                condition,
                if_block,
                else_block,
            } => {
                self.check_if(condition, if_block, else_block);
            }
            StmtType::While { condition, body } => self.check_while(condition, body),
            StmtType::For {
                init,
                condition,
                update,
                body,
            } => {
                self.check_for((init, condition, update, body));
            }
            StmtType::Return(expr) => self.check_ret(expr),
            StmtType::AsmCode(code) => {} // im not sure if there need for checking
            StmtType::InitFunc {
                name,
                args,
                ret_type,
                data,
                generic_types,
            } => {
                self.check_init_func((name, args, ret_type, data, generic_types));
            }
            StmtType::InitStruct(struct_data) => self.check_struct_init(struct_data),
            StmtType::GlobalDecl(global) => {
                if let StmtType::Declaration(decl) = &global.ty {
                    self.global_vars.insert(decl.name.clone(), decl.ty.clone());
                } else {
                    self::panic!("global decl must be a declaration");
                }
            }
            StmtType::ExternFn(stmt) => self.check_extern_fn(stmt),
            StmtType::GenericInitFunc {
                name,
                generic_types,
                args,
                ret_type,
                data,
            } => {}
            StmtType::InitEnum { .. } => {}
            StmtType::Match { expr, variants } => self.check_match(expr, variants),
        }
    }
}
