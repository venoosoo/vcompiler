use std::env::Args;
use std::vec;

use crate::Ir::expr::{EnumExprField, ExprType, Lookup};
use crate::Ir::sem_analysis::SemanticError;
use crate::Ir::stmt::Declaration;
use crate::shared::{build_generic_map, coerce_numeric, is_number, is_numeric, same_signedness};
use crate::{
    Ir::{
        expr::{BinOp, Expr, UnaryOp},
        sem_analysis::Analyzer,
        stmt::Type,
    },
    tokenizer::TokenType,
};

use super::*;

impl<'a> Lookup for Analyzer<'a> {
    fn look_var(&self, name: &String) -> Option<Type> {
        if self.structs.borrow().get(name).is_some() {
            return Some(Type::Struct(name.clone()));
        } else {
            self.lookup(name)
        }
    }
    fn look_unary(&self, op: &UnaryOp, expr: &Box<Expr>) -> Type {
        match op {
            UnaryOp::BitNot => todo!(),
            UnaryOp::Neg => expr.get_type(self),
            UnaryOp::Not => Type::Primitive(TokenType::U8), // boolean
            UnaryOp::GetAddr => Type::Pointer(Box::new(expr.get_type(self))),
        }
    }
    fn look_binary(&self, op: &BinOp, left: &Box<Expr>, right: &Box<Expr>) -> Type {
        let lty = left.get_type(self);
        let rty = right.get_type(self);
        coerce_numeric(&lty, &rty)
    }
    fn look_struct_init(&self, struct_name: &String) -> Type {
        if let Some(_struct_data) = self.structs.borrow().get(struct_name) {
            Type::Struct(struct_name.clone())
        } else {
            self::panic!("Struct {} not found in get_type", struct_name);
        }
    }
    fn look_deref(&self, ptr_expr: &Box<Expr>) -> Type {
        match ptr_expr.get_type(self) {
            Type::Pointer(inner) => *inner,
            _ => self::panic!("Cannot dereference a non-pointer"),
        }
    }
    fn look_addres_of(&self, var_expr: &Box<Expr>) -> Type {
        let ty = var_expr.get_type(self);
        Type::Pointer(Box::new(ty))
    }
    fn look_index(&self, base: &Box<Expr>, index: &Box<Expr>) -> Type {
        let base_ty = base.get_type(self);
        let idx_ty = index.get_type(self);
        if !is_numeric(&idx_ty) {
            self::panic!("Array index must be integer");
        }
        match base_ty {
            Type::Array(elem_ty, _) => *elem_ty,
            Type::Pointer(elem_ty) => *elem_ty,
            _ => self::panic!("Cannot index into non-array type"),
        }
    }
    fn look_struct_member(&self, base: &Box<Expr>, name: &String) -> Type {
        let base_ty = base.get_type(self);
        let base_ty = self.resolve_generic_inst(&base_ty);
        let struct_name = match &base_ty {
            Type::Struct(n) => n.clone(),
            Type::Pointer(inner) => match inner.as_ref() {
                Type::Struct(n) => n.clone(),
                _ => self::panic!("pointer to non-struct"),
            },
            _ => self::panic!("member access on non-struct {:?}", base_ty),
        };
        let struct_data = self.structs.borrow().get(&struct_name).cloned().unwrap();
        let field = struct_data.elements.get(name).unwrap();
        field.ty.clone()
    }
    fn look_call(&self, name: &String, args: &Vec<Expr>, generics: &Vec<Type>) -> Type {
        let func_name = name.clone();

        let vec_func_data: Vec<FuncData> = if generics.len() > 0 {
            let new_name = self.transform_generic_name(&func_name, generics);
            if let Some(existing) = self.functions.get(&new_name) {
                existing.clone()
            } else {
                let func_data = self.functions.get(&func_name).unwrap()[0].clone();
                let generic_map = build_generic_map(&func_data.generic, generics);
                {
                    *self.generics.borrow_mut() = generic_map.clone();
                }
                let new_args: Vec<Declaration> =
                    self.convert_generic_args(&func_data.args, generics, &generic_map);

                let ret_type: Type = match &func_data.return_type {
                    Type::GenericType(name) => {
                        let pos = func_data
                            .generic
                            .iter()
                            .position(|g| g == name)
                            .expect(&format!("non existing generic var: {}", name));
                        generics[pos].clone()
                    }
                    _ => func_data.return_type.clone(),
                };
                vec![FuncData {
                    args: new_args,
                    generic: Vec::new(),
                    return_type: ret_type,
                }]
            }
        } else {
            self.functions.get(&func_name).unwrap().clone()
        };

        let func_data = vec_func_data
            .iter()
            .find(|func| {
                if func.args.len() != args.len() {
                    return false;
                }
                args.iter().enumerate().all(|(index, expr)| {
                    let expr_ty = self.resolve_generic_inst(&expr.get_type(self));
                    let arg_ty = self.resolve_generic_inst(&func.args[index].ty);
                    check_types(&expr_ty, &arg_ty)
                })
            })
            .expect(&format!(
                "no matching overload for function '{}'",
                func_name
            ));

        func_data.return_type.clone()
    }

    fn look_array_init(&self, elements: &Vec<Expr>) -> Type {
        if elements.len() > 0 {
            return elements[0].get_type(self);
        } else {
            Type::Unknown
        }
    }

    fn look_get_enum(&self, base: &String, variant: &String) -> Type {
        Type::Enum(base.clone(), Some(variant.clone()))
    }
}

impl<'a> Analyzer<'a> {
    fn check_num(&mut self, num: &i64, expected_ty: &Type) -> Type {
        match expected_ty {
            Type::Primitive(_) => expected_ty.clone(),
            _ => Type::Primitive(TokenType::I32),
        }
    }
    fn check_var(&mut self, var: &String) -> Type {
        let var_data = self.lookup(var);
        if let Some(var) = var_data {
            var
        } else {
            self.print_error(self.type_to_error(SemanticError::UndeclaredVariable(var.clone())));
            // satisfy return type it wouldnt be compiled because of error anyway
            Type::Primitive(TokenType::I64)
        }
    }

    pub fn resolve_generic_inst(&self, ty: &Type) -> Type {
        match ty {
            Type::GenericInst(_, _) => {
                let mangled = type_name(ty);
                if self.structs.borrow().contains_key(&mangled) {
                    Type::Struct(mangled)
                } else if self.enums.borrow().contains_key(&mangled) {
                    Type::Enum(mangled, None)
                } else {
                    self::panic!("GenericInst not yet monomorphized: {:?}", ty)
                }
            }
            Type::Pointer(inner) => Type::Pointer(Box::new(self.resolve_generic_inst(inner))),
            _ => ty.clone(),
        }
    }

    fn check_binary(
        &mut self,
        op: &BinOp,
        left: &Box<Expr>,
        right: &Box<Expr>,
        expected_ty: &Type,
    ) -> Type {
        let mut l_type = self.check_expr(left, expected_ty);
        let mut r_type: Type = self.check_expr(right, &l_type);

        if matches!(left.ty, ExprType::Number(_)) {
            l_type = r_type.clone();
        }
        if matches!(right.ty, ExprType::Number(_)) {
            r_type = l_type.clone();
        }

        let res = self.check_binary_types(op, l_type, r_type);
        match res {
            Ok(ty) => ty,
            Err(err) => {
                self.print_error(err);
                Type::Primitive(TokenType::I64)
            }
        }
    }

    fn check_unary(&mut self, op: &UnaryOp, expr: &Box<Expr>, expected_ty: &Type) -> Type {
        let mut expr_type = self.check_expr(expr, expected_ty);
        let valid = match op {
            UnaryOp::BitNot => todo!(),
            UnaryOp::Neg => is_number(&expr_type),
            UnaryOp::Not => is_numeric(&expr_type),
            UnaryOp::GetAddr => {
                expr_type = Type::Pointer(Box::new(expr_type));
                true
            } // fix later
        };
        if !valid {
            self.print_error(self.type_to_error(SemanticError::InvalidUnary {
                op: op.clone(),
                ty: expr_type.clone(),
            }));
            return Type::Primitive(TokenType::I64);
        }

        expr_type
    }

    pub fn check_binary_types(&mut self, op: &BinOp, l: Type, r: Type) -> Result<Type, Error> {
        // shared signedness error
        macro_rules! sign_err {
            () => {
                return Err(self.type_to_error(SemanticError::InvalidBinary {
                    op: op.clone(),
                    left: l.clone(),
                    right: r.clone(),
                }))
            };
        }

        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                if matches!(&l, Type::Pointer(_)) && is_number(&r) {
                    return Ok(l);
                }
                if matches!(&r, Type::Pointer(_)) && is_number(&l) {
                    return Ok(r);
                }
                if !is_number(&l) || !is_number(&r) {
                    return Err(self.type_to_error(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    }));
                }
                if !same_signedness(&l, &r) {
                    sign_err!();
                }
                Ok(coerce_numeric(&l, &r))
            }

            BinOp::Mod => {
                if !is_number(&l) || !is_number(&r) {
                    return Err(self.type_to_error(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    }));
                }
                if !same_signedness(&l, &r) {
                    sign_err!();
                }
                Ok(coerce_numeric(&l, &r))
            }

            BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                if !is_numeric(&l) || !is_numeric(&r) {
                    return Err(self.type_to_error(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    }));
                }
                if !same_signedness(&l, &r) {
                    sign_err!();
                }
                Ok(Type::Primitive(TokenType::I32))
            }

            BinOp::Eq | BinOp::Neq => {
                let compatible = (is_numeric(&l) && is_numeric(&r))
                    || l == r
                    || matches!((&l, &r), (Type::Pointer(_), Type::Pointer(_)));
                if !compatible {
                    return Err(self.type_to_error(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    }));
                }
                if is_numeric(&l) && is_numeric(&r) && !same_signedness(&l, &r) {
                    sign_err!();
                }
                Ok(Type::Primitive(TokenType::I32))
            }

            BinOp::And | BinOp::Or => {
                if !is_numeric(&l) || !is_numeric(&r) {
                    return Err(self.type_to_error(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    }));
                }
                if !same_signedness(&l, &r) {
                    sign_err!();
                }
                Ok(Type::Primitive(TokenType::I32))
            }

            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                if !is_number(&l) || !is_number(&r) {
                    return Err(self.type_to_error(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    }));
                }
                if !same_signedness(&l, &r) {
                    sign_err!();
                }
                Ok(coerce_numeric(&l, &r))
            }

            BinOp::ShiftLeft | BinOp::ShiftRight => {
                if !is_number(&l) || !is_number(&r) {
                    return Err(self.type_to_error(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    }));
                }
                // shift amount can be any integer width but must match signedness
                if !same_signedness(&l, &r) {
                    sign_err!();
                }
                Ok(l)
            }
        }
    }

    pub fn transform_generic_name(&self, name: &String, generics: &Vec<Type>) -> String {
        let mut new_generics = Vec::new();
        for i in generics {
            match i {
                Type::GenericType(name) => {}
                _ => new_generics.push(i),
            }
        }
        let mangled = format!(
            "{}__{}",
            name,
            generics
                .iter()
                .map(|t| type_name(t))
                .collect::<Vec<_>>()
                .join("_")
        );
        mangled
    }

    fn resolve_type_with_map(&self, ty: &Type, generic_map: &HashMap<String, Type>) -> Type {
        match ty {
            Type::GenericInst(name, inner_types) => {
                let resolved_inner: Vec<Type> = inner_types
                    .iter()
                    .map(|t| self.resolve_type_with_map(t, generic_map))
                    .collect();

                let normal_name = self.transform_generic_name(name, &resolved_inner);

                if self.structs.borrow().contains_key(&normal_name) {
                    Type::Struct(normal_name)
                } else if self.enums.borrow().contains_key(&normal_name) {
                    Type::Enum(normal_name, None)
                } else {
                    self.ensure_monomorphized(ty)
                }
            }
            Type::GenericType(name) => generic_map
                .get(name)
                .cloned()
                .expect(&format!("Generic type '{}' not found in map!", name)),
            Type::Pointer(inner) => {
                Type::Pointer(Box::new(self.resolve_type_with_map(inner, generic_map)))
            }
            Type::Array(inner, size) => Type::Array(
                Box::new(self.resolve_type_with_map(inner, generic_map)),
                *size,
            ),
            _ => ty.clone(),
        }
    }

    fn convert_generic_arg(
        &self,
        arg: &Declaration,
        arg_ty: &Type,
        generics: &Vec<Type>,
        index: usize,
        generic_map: &HashMap<String, Type>,
    ) -> Declaration {
        Declaration {
            name: arg.name.clone(),
            ty: self.resolve_type_with_map(arg_ty, generic_map),
            initializer: arg.initializer.clone(),
        }
    }

    fn convert_generic_args(
        &self,
        args: &Vec<Declaration>,
        generics: &Vec<Type>,
        generic_map: &HashMap<String, Type>,
    ) -> Vec<Declaration> {
        args.iter()
            .enumerate()
            .map(|(index, arg)| {
                self.convert_generic_arg(arg, &arg.ty, generics, index, generic_map)
            })
            .collect()
    }

    fn check_call(
        &mut self,
        name: &String,
        args: &Vec<Expr>,
        expected_ty: &Type,
        generics: &Vec<Type>,
    ) -> Type {
        let (func_data, func_index) = {
            let data = self.resolve_call(name, args, generics);
            if data.is_none() {
                return Type::Primitive(TokenType::I64);
            } else {
                data.unwrap()
            }
        };
        for (index, generic) in func_data.generic.iter().enumerate() {
            self.generics
                .borrow_mut()
                .insert(generic.clone(), generics[index].clone());
        }
        let args = func_data.args.clone();
        let generic_map = self.generics.borrow().clone();
        let new_args = self.convert_generic_args(&args, generics, &generic_map);
        if func_data.generic.len() > 0 {
            let generic_data = self.generic_func.get(name).unwrap().clone();
            let name = self.transform_generic_name(name, generics);
            if self.functions.get(&name).is_none() {
                let mut return_ty = func_data.return_type.clone();
                match generic_data.ty {
                    StmtType::GenericInitFunc {
                        generic_types,
                        args,
                        ret_type,
                        data,
                        ..
                    } => {
                        let generic_data: HashMap<String, Type> = HashMap::new();
                        return_ty = substitute_type(&ret_type, &generic_types, generics);
                        return_ty = self.ensure_monomorphized(&return_ty);
                    }
                    _ => self::panic!("error"),
                };
                let res_func_data = FuncData {
                    args: new_args.clone(),
                    generic: Vec::new(),
                    return_type: return_ty.clone(),
                };
                self.functions.insert(name.clone(), vec![res_func_data]);
                return return_ty;
            } else {
                let data = self.functions.get(&name).unwrap();
                // TODO: do something here maybe
                return data[0].return_type.clone();
            }
        }
        return func_data.return_type;
    }

    fn check_struct_expr(&mut self, struct_name: &String, fields: &Vec<(String, Expr)>) -> Type {
        let struct_data = self
            .structs
            .borrow()
            .get(struct_name)
            .expect(&format!("no struct with name: {}", struct_name))
            .clone();
        if fields.len() != struct_data.elements.len() {
            self.print_error(self.type_to_error(SemanticError::StructCountMismatch {
                struct_name: struct_name.clone(),
                expected: struct_data.elements.len(),
                got: fields.len(),
            }));
        }
        for (arg_name, arg) in fields.iter() {
            let res = struct_data.elements.get(arg_name);
            if let Some(struct_arg) = res {
                let arg_type = if matches!(arg.ty, ExprType::Number(_)) {
                    self.check_expr(arg, &struct_arg.ty)
                } else {
                    arg.get_type(self)
                };
                if !check_types(&struct_arg.ty, &arg_type) {
                    self.print_error(self.type_to_error(SemanticError::StructTypeMismatch {
                        struct_name: struct_name.clone(),
                        expected: struct_arg.ty.clone(),
                        got: arg_type,
                    }));
                }
            } else {
                self.print_error(self.type_to_error(SemanticError::StructNameNotFound {
                    struct_name: struct_name.clone(),
                    got: arg_name.clone(),
                }));
            }
        }
        Type::Struct(struct_name.to_string())
    }

    fn check_struct_member(&mut self, base: &Box<Expr>, name: &String, expected_ty: &Type) -> Type {
        let base = self.check_expr(base, expected_ty);
        let base = self.ensure_monomorphized(&base);
        match base {
            Type::Struct(struct_name) => {
                let res = self.structs.borrow().get(&struct_name).cloned();
                if let Some(struct_data) = res {
                    let name_res = struct_data.elements.get(name);
                    if let Some(arg) = name_res {
                        return arg.ty.clone();
                    } else {
                        self.print_error(self.type_to_error(SemanticError::StructNameNotFound {
                            struct_name,
                            got: name.clone(),
                        }));
                        return Type::Unknown;
                    }
                } else {
                    self.print_error(
                        self.type_to_error(SemanticError::UndeclaredStruct(struct_name)),
                    );
                    return Type::Unknown;
                }
            }
            _ => {
                self.print_error(self.type_to_error(SemanticError::NotAStruct(base.clone())));
                return Type::Unknown;
            }
        }
    }

    fn check_deref(&mut self, expr: &Box<Expr>, expected_ty: &Type) -> Type {
        let expr_ty = self.check_expr(expr, expected_ty);
        match expr_ty {
            Type::Pointer(ty) => {
                return *ty.clone();
            }
            _ => {
                self.print_error(self.type_to_error(SemanticError::NotAPointer(expr_ty)));
                Type::Unknown
            }
        }
    }

    fn check_addres_of(&mut self, expr: &Box<Expr>, expected_ty: &Type) -> Type {
        let expr_ty = self.check_expr(expr, expected_ty);
        return Type::Pointer(Box::new(expr_ty));
    }

    fn check_index(&mut self, base: &Box<Expr>, index: &Box<Expr>, expected_ty: &Type) -> Type {
        let base_ty = self.check_expr(base, expected_ty);
        let index_ty = self.check_expr(index, expected_ty);

        if !is_numeric(&index_ty) {
            self.print_error(
                self.type_to_error(SemanticError::InvalidArrayIndex(index_ty.clone())),
            );
        }

        match base_ty {
            Type::Array(arr_type, size) => *arr_type.clone(),
            Type::Pointer(ty) => *ty,
            _ => {
                self.print_error(self.type_to_error(SemanticError::NonArrayIndex(base_ty.clone())));
                Type::Unknown
            }
        }
    }

    fn check_array_init(&mut self, elements: &Vec<Expr>, expected_ty: &Type) -> Type {
        if elements.is_empty() {
            self.print_error(self.type_to_error(SemanticError::EmptyArray));
            return Type::Unknown;
        }

        let first_ty = expected_ty;
        for elem in elements.iter().skip(1) {
            let elem_ty = self.check_expr(elem, expected_ty);
            if !check_types(&first_ty, &elem_ty) {
                self.print_error(self.type_to_error(SemanticError::TypeMismatch {
                    expected: first_ty.clone(),
                    got: elem_ty,
                }));
            }
        }

        Type::Array(Box::new(first_ty.clone()), elements.len())
    }

    fn check_size_of(&mut self, expr: &Type) -> Type {
        Type::Primitive(TokenType::I64)
    }

    fn check_gen_enum(
        &mut self,
        base: &String,
        value: &Vec<EnumExprField>,
        variant: &String,
    ) -> Type {
        let enum_data = self
            .enums
            .borrow()
            .get(base)
            .cloned()
            .expect(&format!("no enums with name: {}\n{:?}", base, self.enums));
        let variant_data = enum_data
            .variants
            .get(variant)
            .expect(&format!("in struct: {} no variant {}", base, variant));
        for (index, arg) in variant_data.args.iter().enumerate() {
            let enum_field = &value[index];
            if !check_types(&enum_field.expr.get_type(self), &arg.ty) {
                self::panic!("debil²")
            }
        }
        Type::Enum(base.clone(), None)
    }

    fn check_cast(&mut self, expr: &Expr, ty: &Type) -> Type {
        if expr.get_type(self) == *ty {
            self.print_error(self.type_to_error(SemanticError::CastError {
                before: expr.get_type(self),
                after: ty.clone(),
            }));
            ty.clone()
        } else {
            ty.clone()
        }
    }

    pub fn check_expr(&mut self, expr: &Expr, expected_ty: &Type) -> Type {
        match &expr.ty {
            ExprType::Number(num) => self.check_num(num, expected_ty),
            ExprType::Float(num) => self::panic!("not implemented"),
            ExprType::Variable(var) => self.check_var(var),
            ExprType::Binary { op, left, right } => self.check_binary(op, left, right, expected_ty),
            ExprType::Unary { op, expr } => self.check_unary(op, expr, expected_ty),
            ExprType::Call {
                name,
                args,
                generics,
            } => {
                let ty = self.check_call(name, args, expected_ty, generics);
                return ty;
            }
            ExprType::StructInit {
                struct_name_ty,
                fields,
            } => self.check_struct_expr(struct_name_ty, fields),
            ExprType::StructMember { base, name } => {
                self.check_struct_member(base, name, expected_ty)
            }
            ExprType::Deref(expr) => self.check_deref(expr, expected_ty),
            ExprType::Index { base, index } => self.check_index(base, index, expected_ty),
            ExprType::ArrayInit { elements } => self.check_array_init(elements, expected_ty),
            ExprType::SizeOf { ty } => self.check_size_of(ty),
            ExprType::String { str } => {
                return Type::Array(Box::new(Type::Primitive(TokenType::U8)), str.len() + 1);
            }
            ExprType::GetEnum {
                base,
                value,
                variant,
            } => self.check_gen_enum(base, value, variant),
            ExprType::Cast { expr, ty } => self.check_cast(expr, ty),
        }
    }
}
