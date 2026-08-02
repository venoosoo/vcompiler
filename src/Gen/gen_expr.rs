use std::fmt::format;
use std::{dbg, vec};

use indexmap::IndexMap;

use crate::Ir::expr::{self, BinOp, EnumExprField, Expr, ExprType, Lookup, UnaryOp};
use crate::Ir::r#gen;
use crate::Ir::shared::TypeContext;
use crate::Ir::stmt::{Declaration, EnumVariant, StructField};
use crate::shared::{
    arg_pos, build_generic_map, coerce_numeric, is_numeric, is_unsigned, transform_generic_name,
};

use super::*;

impl Lookup for Gen {
    fn look_var(&self, name: &String) -> Option<Type> {
        if self.structs.borrow().get(name).is_some() {
            return Some(Type::Struct(name.clone()));
        }
        if self.enums.borrow().get(name).is_some() {
            return Some(Type::Enum(name.clone(), None));
        } else {
            let var = self.lookup_var(name);
            return Some(var.var_type.clone());
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

        match (&left.ty, &right.ty) {
            (ExprType::Number(_), _) => rty,
            (_, ExprType::Number(_)) => lty,
            _ => coerce_numeric(&lty, &rty),
        }
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
            _ => self::panic!(
                "Cannot dereference a non-pointer: {:?}",
                ptr_expr.get_type(self)
            ),
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
            self::panic!("Array index must be a number");
        }
        match base_ty {
            Type::Array(elem_ty, _) => *elem_ty,
            Type::Pointer(elem_ty) => *elem_ty,
            _ => base_ty,
        }
    }
    fn look_struct_member(&self, base: &Box<Expr>, name: &String) -> Type {
        let base_ty = base.get_type(self);
        let base_ty = self.resolve_generic_inst(&base_ty);
        let struct_name = match &base_ty {
            Type::Struct(n) => n.clone(),
            Type::Pointer(inner) => match inner.as_ref() {
                Type::Struct(n) => n.clone(),
                _ => self::panic!("pointer to non-struct: {:?}", inner),
            },
            _ => self::panic!("member access on non-struct: {:?}", base_ty),
        };
        let struct_data = self.structs.borrow().get(&struct_name).cloned().unwrap();
        let field = struct_data.elements.get(name).unwrap();
        field.ty.clone()
    }
    fn look_call(&self, name: &String, args: &Vec<Expr>, generics: &Vec<Type>) -> Type {
        let func_name = name.clone();

        let vec_func_data: Vec<FuncData> = if generics.len() > 0 {
            let (func_data, overload_pos) = self.resolve_call(name, args, generics).unwrap();
            let new_name = transform_generic_name(&func_name, generics, overload_pos as i64);
            if let Some(existing) = self.functions.get(&new_name) {
                existing.clone()
            } else {
                let generic_map = build_generic_map(&func_data.generic, generics);
                {
                    *self.generics.borrow_mut() = generic_map.clone();
                }
                let new_args: Vec<Declaration> =
                    self.convert_generic_args(&func_data.args, generics, &generic_map, 0);

                let ret_type: Type = match &func_data.return_type {
                    Type::GenericType(name) => {
                        let pos = func_data.generic.iter().position(|g| g == name).unwrap();
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

impl Gen {
    fn gen_expr_binop(
        &mut self,
        op: &BinOp,
        left_reg: &str,
        right_reg: &str,
        expected_type: &Type,
    ) {
        let is_unsigned = is_unsigned(expected_type);
        match op {
            BinOp::BitAnd => {
                self.emit_func_data(format!("    and {}, {}", left_reg, right_reg));
            }
            BinOp::BitOr => {
                self.emit_func_data(format!("    or {}, {}", left_reg, right_reg));
            }
            BinOp::BitXor => {
                self.emit_func_data(format!("    xor {}, {}", left_reg, right_reg));
            }
            BinOp::ShiftLeft => {
                // shift amount must be in cl
                if is_unsigned {
                    self.emit(format!("    mov rcx, {}", right_reg));
                    self.emit(format!("    shl {}, cl", left_reg));
                } else {
                    self.emit(format!("    mov rcx, {}", right_reg));
                    self.emit(format!("    sal {}, cl", left_reg));
                }
            }
            BinOp::ShiftRight => {
                if is_unsigned {
                    self.emit(format!("    mov rcx, {}", right_reg));
                    self.emit(format!("    shr {}, cl", left_reg));
                } else {
                    self.emit(format!("    mov rcx, {}", right_reg));
                    self.emit(format!("    sar {}, cl", left_reg));
                }
            }
            BinOp::Add => {
                self.emit_func_data(format!("    add {}, {}", left_reg, right_reg));
            }
            BinOp::Sub => {
                self.emit_func_data(format!("    sub {}, {}", left_reg, right_reg));
            }
            BinOp::Mul => {
                self.emit_func_data(format!("    imul {}, {}", left_reg, right_reg));
            }
            BinOp::Div => {
                let divisor_reg = self.reg_for_size("r10", expected_type).unwrap();

                self.emit_func_data(format!("    mov {}, {}", divisor_reg, right_reg));
                self.emit_func_data("    push rdx".to_string());

                if is_unsigned {
                    let rdx_reg = self.reg_for_size("rdx", expected_type).unwrap();
                    self.emit_func_data(format!("    xor {}, {}", rdx_reg, rdx_reg));
                    self.emit_func_data(format!("    div {}", divisor_reg));
                } else {
                    // SIGNED PATH
                    if self.type_size(expected_type) == 8 {
                        self.emit_func_data("    cqo".to_string());
                    } else if self.type_size(expected_type) == 4 {
                        self.emit_func_data("    cdq".to_string());
                    } else {
                        self.emit_func_data("    cwd".to_string());
                    }
                    self.emit_func_data(format!("    idiv {}", divisor_reg));
                }

                self.emit_func_data(format!("    pop rdx"));
            }
            BinOp::Mod => {
                let divisor_reg = self.reg_for_size("r10", expected_type).unwrap();

                self.emit_func_data(format!("    mov {}, {}", divisor_reg, right_reg));
                self.emit_func_data("    push rdx".to_string());

                if is_unsigned {
                    let rdx_reg = self.reg_for_size("rdx", expected_type).unwrap();
                    self.emit_func_data(format!("    xor {}, {}", rdx_reg, rdx_reg));
                    self.emit_func_data(format!("    div {}", divisor_reg));
                } else {
                    // SIGNED PATH
                    if self.type_size(expected_type) == 8 {
                        self.emit_func_data("    cqo".to_string());
                    } else if self.type_size(expected_type) == 4 {
                        self.emit_func_data("    cdq".to_string());
                    } else {
                        self.emit_func_data("    cwd".to_string());
                    }
                    self.emit_func_data(format!("    idiv {}", divisor_reg));
                }

                let remainder_reg = self.reg_for_size("rdx", expected_type).unwrap();
                self.emit_func_data(format!("    mov {}, {}", left_reg, remainder_reg));

                self.emit_func_data("    pop rdx".to_string());
            }
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                self.emit_func_data(format!("    cmp {}, {}", left_reg, right_reg));
                let set_instr = match op {
                    BinOp::Eq => "sete",
                    BinOp::Neq => "setne",
                    BinOp::Lt => {
                        if is_unsigned {
                            "setb"
                        } else {
                            "setl"
                        }
                    }
                    BinOp::Lte => {
                        if is_unsigned {
                            "setbe"
                        } else {
                            "setle"
                        }
                    }
                    BinOp::Gt => {
                        if is_unsigned {
                            "seta"
                        } else {
                            "setg"
                        }
                    }
                    BinOp::Gte => {
                        if is_unsigned {
                            "setae"
                        } else {
                            "setge"
                        }
                    }
                    _ => unreachable!(),
                };
                self.emit_func_data(format!("    {} al", set_instr));
                if left_reg != "al" {
                    self.emit_func_data(format!("    movzx {}, al", left_reg));
                }
            }
            BinOp::And => {
                unreachable!()
            }
            BinOp::Or => {
                let left_byte = self
                    .reg_for_size(left_reg, &Type::Primitive(TokenType::U8))
                    .unwrap();
                let right_byte = self
                    .reg_for_size(right_reg, &Type::Primitive(TokenType::U8))
                    .unwrap();
                self.emit_func_data(format!("    cmp {}, 0", left_reg));
                self.emit_func_data(format!("    setne {}", left_byte));
                self.emit_func_data(format!("    cmp {}, 0", right_reg));
                self.emit_func_data(format!("    setne {}", right_byte));
                self.emit_func_data(format!("    or {}, {}", left_byte, right_byte));
                self.emit_func_data(format!("    movzx {}, {}", left_reg, left_byte));
            }
        }
    }

    fn push_result(&mut self) {
        self.emit_func_data("    push rax".to_string());
    }
    fn pop_into(&mut self, reg: &str) {
        self.emit_func_data(format!("    pop {}", reg));
    }

    fn gen_expr_num(&mut self, num: &i64, expected_type: &Type) -> String {
        let expected_type = match expected_type {
            Type::GenericType(name) => {
                let map = self.generics.borrow();

                &map.get(name).cloned().unwrap()
            }
            _ => expected_type,
        };
        self.emit_func_data(format!("    mov rax, {}", num));
        "rax".to_string()
    }

    fn var_return(&mut self, var_data: &VarData) {
        match &var_data.var_type {
            Type::Primitive(_) => {
                let sized_rax = self.reg_for_size("rax", &var_data.var_type).unwrap();
                self.emit_func_data(format!(
                    "    mov {}, {} [rbp - {}]",
                    sized_rax,
                    self.get_word(&var_data.var_type),
                    var_data.stack_pos
                ));
            }
            Type::Pointer(_) => {
                self.emit_func_data(format!("    mov rax, [rbp - {}]", var_data.stack_pos));
            }
            _ => {
                // struct/array — load address
                self.emit_func_data(format!("    lea rax, [rbp - {}]", var_data.stack_pos));
            }
        }
    }

    fn gen_expr_var(&mut self, var_name: &String, expected_type: &Type) -> String {
        let var_data = self.lookup_var(var_name).clone();
        if var_data.global_flag {
            match var_data.var_type {
                Type::Primitive(_) | Type::Pointer(_) => {
                    let sized_rax = self.reg_for_size("rax", &var_data.var_type).unwrap();
                    self.emit_func_data(format!("    mov {}, [rel {}]", sized_rax, var_name));
                }
                _ => {
                    // struct/array — load address
                    self.emit_func_data(format!("    lea rax, [rel {}]", var_name));
                }
            }
            return "rax".to_string();
        }

        self.var_return(&var_data);

        "rax".to_string()
    }

    // TODO: make this better
    fn var_return_addr(&mut self, var_data: &VarData, expected_type: &Type) {
        match &var_data.var_type {
            Type::Primitive(_) => {
                let actual_size = self.type_size(&var_data.var_type);
                let expected_size = self.type_size(expected_type);
                if expected_size > actual_size {
                    let src_word = self.get_word(&var_data.var_type);
                    self.emit_func_data(format!(
                        "    movsx rax, {} [rbp - {}]",
                        src_word, var_data.stack_pos
                    ));
                } else {
                    let sized_rax = self.reg_for_size("rax", &var_data.var_type).unwrap();
                    self.emit_func_data(format!(
                        "    mov {}, {} [rbp - {}]",
                        sized_rax,
                        self.get_word(&var_data.var_type),
                        var_data.stack_pos
                    ));
                }
            }
            Type::Pointer(_) => {
                self.emit_func_data(format!("    mov rax, [rbp - {}]", var_data.stack_pos));
            }
            Type::Enum(ty, _) => {
                self.emit_func_data(format!("    lea rax, [rbp - {}]", var_data.stack_pos));
            }
            _ => {
                // struct/array — load address
                self.emit_func_data(format!("    lea rax, [rbp - {}]", var_data.stack_pos));
            }
        }
    }

    // TODO: make this better
    pub fn gen_expr_var_addr(&mut self, var_name: &String, expected_type: &Type) -> String {
        let var_data = self.lookup_var(var_name).clone();
        if var_data.global_flag {
            match var_data.var_type {
                Type::Primitive(_) | Type::Pointer(_) => {
                    let sized_rax = self.reg_for_size("rax", &var_data.var_type).unwrap();
                    self.emit_func_data(format!("    mov {}, [rel {}]", sized_rax, var_name));
                }
                _ => {
                    // struct/array — load address
                    self.emit_func_data(format!("    lea rax, [rel {}]", var_name));
                }
            }
            return "rax".to_string();
        }
        self.var_return_addr(&var_data, expected_type);

        "rax".to_string()
    }

    fn gen_expr_binary(
        &mut self,
        data: (&BinOp, &Box<Expr>, &Box<Expr>),
        expected_type: &Type,
    ) -> String {
        let (op, left, right) = data;
        let left_reg = self.reg_for_size("rax", &expected_type).unwrap();
        let right_reg = self.reg_for_size("rbx", &expected_type).unwrap();

        let left_ty = left.get_type(self);
        let is_op_add = matches!(op, BinOp::Add);
        let right_ty = right.get_type(self);
        let left_ty = self.ensure_monomorphized(&left_ty);
        let right_ty = self.ensure_monomorphized(&right_ty);

        // the and exception
        if *op == BinOp::And {
            let id = self.get_id();
            self.emit_func_data(format!("and_{id}:"));
            self.eval_expr(left, expected_type);
            self.emit_func_data(format!("    cmp {}, 0", left_reg));
            self.emit_func_data(format!("    je and_end_{}", id));
            self.eval_expr(right, expected_type);
            self.emit_func_data(format!("    cmp {}, 0", right_reg));
            self.emit_func_data(format!("and_end_{id}:"));

            return left_reg;
        }

        self.eval_expr(right, expected_type);
        if is_op_add {
            if let Type::Pointer(inner_ty) = &left_ty {
                let inner_size = self.type_size(&inner_ty);
                if inner_size > 1 {
                    self.emit_func_data(format!("    imul rax, rax, {}", inner_size));
                }
            }
        }
        self.push_result();
        self.eval_expr(left, expected_type);
        if is_op_add {
            if let Type::Pointer(inner_ty) = &right_ty {
                let inner_size = self.type_size(&inner_ty);
                if inner_size > 1 {
                    self.emit_func_data(format!("    imul rax, rax, {}", inner_size));
                }
            }
        }
        self.pop_into("rbx");

        self.gen_expr_binop(op, &left_reg, &right_reg, expected_type);

        left_reg
    }

    fn gen_expr_unary(&mut self, op: &UnaryOp, expr: &Box<Expr>, expected_type: &Type) -> String {
        match op {
            UnaryOp::Neg => {
                self.eval_expr(expr, expected_type);
                let sized = self.reg_for_size("rax", expected_type).unwrap();
                self.emit_func_data(format!("    neg {}", sized));
            }
            UnaryOp::Not => {
                self.eval_expr(expr, expected_type);
                let sized = self.reg_for_size("rax", expected_type).unwrap();
                self.emit_func_data(format!("    cmp {}, 0", sized));
                self.emit_func_data("    sete al".to_string());
                self.emit_func_data(format!("    movzx {}, al", sized));
            }
            UnaryOp::GetAddr => {
                self.gen_expr_addres_of(expr);
            }
            UnaryOp::BitNot => {
                todo!();
            }
        }
        "rax".to_string()
    }

    fn resolve_type_with_map(
        &self,
        ty: &Type,
        generic_map: &HashMap<String, Type>,
        overload_pos: i64,
    ) -> Type {
        match ty {
            Type::GenericInst(name, inner_types) => {
                let resolved_inner: Vec<Type> = inner_types
                    .iter()
                    .map(|t| self.resolve_type_with_map(t, generic_map, overload_pos))
                    .collect();

                let normal_name = transform_generic_name(name, &resolved_inner, -1);
                if self.structs.borrow().contains_key(&normal_name) {
                    Type::Struct(normal_name)
                } else if self.enums.borrow().contains_key(&normal_name) {
                    Type::Enum(normal_name, None)
                } else {
                    dbg!(&ty);
                    dbg!(&self.enums);
                    self::panic!("Generic struct not monomorphized yet: {}", normal_name);
                }
            }
            Type::GenericType(name) => generic_map
                .get(name)
                .cloned()
                .expect(&format!("Generic type '{}' not found in map!", name)),
            Type::Pointer(inner) => Type::Pointer(Box::new(self.resolve_type_with_map(
                inner,
                generic_map,
                overload_pos,
            ))),
            Type::Array(inner, size) => Type::Array(
                Box::new(self.resolve_type_with_map(inner, generic_map, overload_pos)),
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
        overload_pos: i64,
        generic_map: &HashMap<String, Type>,
    ) -> Declaration {
        Declaration {
            name: arg.name.clone(),
            ty: self.resolve_type_with_map(arg_ty, generic_map, overload_pos),
            initializer: arg.initializer.clone(),
        }
    }

    fn convert_generic_args(
        &self,
        args: &Vec<Declaration>,
        generics: &Vec<Type>,
        generic_map: &HashMap<String, Type>,
        overload_pos: i64,
    ) -> Vec<Declaration> {
        args.iter()
            .enumerate()
            .map(|(index, arg)| {
                self.convert_generic_arg(arg, &arg.ty, generics, index, overload_pos, generic_map)
            })
            .collect()
    }

    fn push_arg(&mut self, pos: usize, ty: &Type, rval: &str) {
        let arg_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        let arg: Option<String> = {
            if pos < 6 {
                Some(self.reg_for_size(arg_regs[pos], ty).unwrap())
            } else {
                self.alloc_type(ty);
                self.emit_func_data(format!("    push {}", rval));
                None
            }
        };
        match arg {
            Option::Some(data) => {
                self.emit_func_data(format!("    mov {}, {}", data, rval));
            }
            _ => {}
        }
    }

    pub fn arg_count(&mut self, is_rvo: bool, args: &Vec<Declaration>) -> usize {
        let mut count = 0;
        for i in args.iter() {
            match &i.ty {
                Type::Struct(name) => {
                    let struct_data = self.structs.borrow().get(name).cloned().unwrap();
                    if struct_data.size <= 8 {
                        count += 1
                    } else if struct_data.size <= 16 {
                        count += 2
                    }
                }
                Type::Enum(name, _) => {
                    let enum_data = self.enums.borrow().get(name).cloned().unwrap();
                    if enum_data.size <= 8 {
                        count += 1
                    } else if enum_data.size <= 16 {
                        count += 2
                    }
                }
                _ => {
                    count += 1;
                }
            }
        }
        if is_rvo {
            count += 1;
            count
        } else {
            count
        }
    }

    fn gen_args(
        &mut self,
        args: &Vec<Expr>,
        func_data: &FuncData,
        generics: &Vec<Type>,
        new_args: &Vec<Declaration>,
        is_rvo: bool,
    ) -> usize {
        let mut arg_index = self.arg_count(is_rvo, new_args);

        let mut space_taken = 0;
        for (index, arg) in args.iter().enumerate().rev() {
            let arg_type = new_args[index].ty.clone();
            self.eval_expr(&arg, &arg_type);
            match arg_type {
                Type::Enum(ref name, _) => {
                    let enum_data = self.enums.borrow().get(name).cloned().unwrap();
                    if enum_data.size <= 8 {
                        self.emit_func_data(format!("    mov rax, [rax]"));
                        arg_index -= 1;
                        self.push_arg(arg_index, &arg_type, "rax");
                    } else if enum_data.size <= 16 && arg_index < 6 && arg_index >= 1 {
                        self.emit_func_data(format!("    mov r10, rax"));
                        self.emit_func_data(format!("    mov rax, [r10 + 8]"));
                        arg_index -= 1;
                        self.push_arg(arg_index, &arg_type, "rax");
                        self.emit_func_data(format!("    mov rax, [r10]"));
                        arg_index -= 1;
                        self.push_arg(arg_index, &arg_type, "rax");
                    } else {
                        let chunks = (enum_data.size + 7) / 8;
                        let remainder = enum_data.size % 8;
                        let full = if remainder > 0 { chunks - 1 } else { chunks };
                        if remainder > 0 {
                            self.emit_func_data(format!("    xor r10, r10"));
                            self.emit_func_data(format!("    sub rsp, 16"));
                            space_taken += 16;
                            match remainder {
                                4 => self.emit_func_data(format!(
                                    "    mov r10d, [rax + {}]",
                                    (chunks - 1) * 8
                                )),
                                2 => self.emit_func_data(format!(
                                    "    mov r10w, [rax + {}]",
                                    (chunks - 1) * 8
                                )),
                                1 => self.emit_func_data(format!(
                                    "    mov r10b, [rax + {}]",
                                    (chunks - 1) * 8
                                )),
                                _ => {
                                    todo!()
                                }
                            }
                            self.emit_func_data(format!("    mov [rsp], r10"));
                        }
                        for i in (0..full).rev() {
                            self.emit_func_data(format!("    push qword [rax + {}]", i * 8));
                        }
                        space_taken += full * 8
                    }
                }
                Type::Struct(ref name) => {
                    let struct_data = self.structs.borrow().get(name).cloned().unwrap();
                    if struct_data.size <= 8 {
                        self.emit_func_data(format!("    mov rax, [rax]"));
                        arg_index -= 1;
                        self.push_arg(arg_index, &arg_type, "rax");
                    } else if struct_data.size <= 16 && arg_index < 6 && arg_index >= 1 {
                        self.emit_func_data(format!("    mov r10, rax"));
                        self.emit_func_data(format!("    mov rax, [r10 + 8]"));
                        arg_index -= 1;
                        self.push_arg(arg_index, &arg_type, "rax");
                        self.emit_func_data(format!("    mov rax, [r10]"));
                        arg_index -= 1;
                        self.push_arg(arg_index, &arg_type, "rax");
                    } else {
                        let chunks = (struct_data.size + 7) / 8;
                        let remainder = struct_data.size % 8;
                        let full = if remainder > 0 { chunks - 1 } else { chunks };

                        if remainder > 0 {
                            self.emit_func_data(format!("    xor r10, r10"));
                            self.emit_func_data(format!("    sub rsp, 16"));
                            space_taken += 16;
                            match remainder {
                                4 => self.emit_func_data(format!(
                                    "    mov r10d, [rax + {}]",
                                    (chunks - 1) * 8
                                )),
                                2 => self.emit_func_data(format!(
                                    "    mov r10w, [rax + {}]",
                                    (chunks - 1) * 8
                                )),
                                1 => self.emit_func_data(format!(
                                    "    mov r10b, [rax + {}]",
                                    (chunks - 1) * 8
                                )),
                                _ => {}
                            }
                            self.emit_func_data(format!("    mov [rsp], r10"));
                        }

                        for i in (0..full).rev() {
                            self.emit_func_data(format!("    push qword [rax + {}]", i * 8));
                        }
                        if full % 2 == 1 {
                            self.emit_func_data(format!("    sub rsp, 8"));
                            space_taken += 8;
                        }
                        space_taken += full * 8;
                    }
                }
                _ => {
                    let rval = self.reg_for_size("rax", &arg_type).unwrap();
                    arg_index -= 1;
                    self.push_arg(arg_index, &arg_type, &rval);
                }
            }
        }
        return space_taken;
    }

    fn gen_call(
        &mut self,
        name: &String,
        args: &Vec<Expr>,
        func_data: &FuncData,
        overload_pos: usize,
        generics: &Vec<Type>,
    ) -> String {
        let args = args.clone();
        let mut name = name.clone();
        let mut generic_copy = generics.clone();
        let mut is_rvo = false; // return value optimization

        for (index, generic) in func_data.generic.iter().enumerate() {
            let ty = &generic_copy[index].clone();
            match ty {
                Type::GenericType(name) => {
                    let ty = {
                        let map = self.generics.borrow();
                        if let Some(generic_ty) = map.get(name) {
                            generic_ty.clone()
                        } else if self.structs.borrow().contains_key(name) {
                            Type::Struct(name.clone())
                        } else if self.enums.borrow().contains_key(name) {
                            Type::Enum(name.clone(), None)
                        } else {
                            self::panic!(
                                "Type resolution failed: '{}' is not a known generic, struct, or enum.",
                                name
                            );
                        }
                    };
                    generic_copy[index] = ty;
                }
                _ => {}
            }
            let mut map = self.generics.borrow_mut();
            map.insert(generic.clone(), generic_copy[index].clone());
        }

        let new_args = {
            let map = self.generics.borrow();
            self.convert_generic_args(&func_data.args, generics, &map, overload_pos as i64)
        };

        match &self.ensure_monomorphized(&func_data.return_type) {
            Type::Struct(_) | Type::Enum(_, _) => {
                self.emit_func_data(format!("    lea rdi, [rbp - {}]", self.stack_pos));
                is_rvo = true;
            }
            _ => {}
        }

        let stack_pos_save = self.stack_pos;
        let saved_ret_type = self.current_return_type.clone();
        self.stack_pos = 0;

        let space_taken = self.gen_args(&args, func_data, &generic_copy, &new_args, is_rvo);

        if func_data.generic.len() > 0 {
            let generic_candidates = self.generic_func.get(&name).unwrap().clone();
            let generic_data = generic_candidates
                .iter()
                .find(|stmt| match &stmt.ty {
                    StmtType::GenericInitFunc {
                        generic_types,
                        args,
                        ..
                    } => {
                        args.len() == func_data.args.len()
                            && generic_types.len() == func_data.generic.len()
                            && args.iter().zip(func_data.args.iter()).all(
                                |(decl_arg, resolved_arg)| {
                                    let decl_ty = self.resolve_generic_inst(&decl_arg.ty);
                                    let resolved_ty = self.resolve_generic_inst(&resolved_arg.ty);
                                    check_types(&decl_ty, &resolved_ty)
                                },
                            )
                    }
                    _ => false,
                })
                .expect(&format!(
                    "no matching generic definition for function '{}'",
                    name
                ))
                .clone();

            name = transform_generic_name(&name, &generic_copy, overload_pos as i64);
            if self.functions.get(&name).is_none() {
                let ret_type = self.resolve_type_with_map(
                    &func_data.return_type,
                    &self.generics.borrow(),
                    overload_pos as i64,
                );
                let res_func_data = FuncData {
                    args: new_args.clone(),
                    generic: Vec::new(),
                    return_type: ret_type,
                };
                self.functions.insert(name.clone(), vec![res_func_data]);
                match generic_data.ty {
                    StmtType::GenericInitFunc {
                        generic_types,
                        args,
                        ret_type,
                        data,
                        ..
                    } => {
                        let mut generic_data: HashMap<String, Type> = HashMap::new();
                        for i in 0..generic_types.len() {
                            let generic_var = generic_types[i].clone();
                            let generic_ty = generics[i].clone();
                            generic_data.insert(generic_var, generic_ty);
                        }
                        self.gen_func((&name, &new_args, &ret_type, &data, &generic_data));
                    }
                    _ => self::panic!("error"),
                };
            }
        }

        if self.functions.get(&name).unwrap().len() > 1 {
            self.emit_func_data(format!("    call {}___{}", name, overload_pos));
        } else {
            self.emit_func_data(format!("    call {}", name));
        }
        if space_taken > 0 {
            self.emit_func_data(format!("    add rsp, {}", space_taken));
        }
        self.stack_pos = stack_pos_save;
        self.current_return_type = saved_ret_type;
        return "rax".to_string();
    }

    pub fn generic_to_ty(&self, ty: &Type, type_map: &HashMap<String, Type>) -> Type {
        match ty {
            Type::GenericType(name) => type_map.get(name).cloned().unwrap_or(ty.clone()),
            Type::Array(arr_ty, size) => {
                let res = self.generic_to_ty(arr_ty, type_map);
                Type::Array(Box::new(res), *size)
            }
            Type::Pointer(ptr_ty) => {
                let res = self.generic_to_ty(ptr_ty, type_map);
                Type::Pointer(Box::new(res))
            }
            Type::GenericInst(name, type_args) => {
                let resolved_args: Vec<Type> = type_args
                    .iter()
                    .map(|arg| self.generic_to_ty(arg, type_map))
                    .collect();
                Type::GenericInst(name.clone(), resolved_args)
            }
            _ => ty.clone(),
        }
    }

    fn resolve_generic(
        &self,
        expr_ty: &Type,
        field_ty: &Type,
        type_map: &mut HashMap<String, Type>,
    ) {
        match field_ty {
            Type::GenericType(param_name) => {
                type_map.insert(param_name.clone(), expr_ty.clone());
            }
            Type::Array(ty, size) => {
                self.resolve_generic(expr_ty, ty, type_map);
            }
            Type::Pointer(ty) => {
                self.resolve_generic(expr_ty, ty, type_map);
            }
            _ => {}
        }
    }

    fn gen_generic_struct(&mut self, fields: &Vec<(String, Expr)>, struct_name: &String) -> String {
        let struct_data = self.structs.borrow().get(struct_name).unwrap().clone();

        let mut type_map: HashMap<String, Type> = HashMap::new();
        for (index, (_, field)) in struct_data.elements.iter().enumerate() {
            let expr_ty = fields[index].1.get_type(self);
            self.resolve_generic(&expr_ty, &field.ty, &mut type_map);
        }

        let mut new_elements = IndexMap::new();
        let mut offset = 0;
        for (field_name, field_data) in struct_data.elements.iter() {
            let ty = self.generic_to_ty(&field_data.ty, &type_map);
            let new_field = StructField {
                ty: ty.clone(),
                offset: offset,
                name: field_data.name.clone(),
            };
            offset += self.type_size(&ty);
            new_elements.insert(field_name.clone(), new_field);
        }

        let type_args: Vec<String> = struct_data
            .generic_type
            .iter()
            .map(|param| type_map.get(param).map(type_name).unwrap_or(param.clone()))
            .collect();
        let name = format!("{}__{}", struct_data.name, type_args.join("_"));

        let fields: Vec<StructField> = new_elements.values().cloned().collect();
        let size = self.compute_struct_size(&fields);

        // Register if not already monomorphized
        if !self.structs.borrow().contains_key(&name) {
            let new_data = StructData {
                name: name.clone(),
                generic_type: Vec::new(),
                elements: new_elements,
                size,
            };
            self.structs.borrow_mut().insert(name.clone(), new_data);
        }

        name
    }

    fn gen_expr_struct_init(
        &mut self,
        fields: &Vec<(String, Expr)>,
        struct_name: &String,
    ) -> String {
        let mut struct_data = self
            .structs
            .borrow()
            .get(struct_name)
            .expect("Unknown struct")
            .clone();
        if struct_data.generic_type.len() > 0 {
            let name = self.gen_generic_struct(fields, struct_name);
            struct_data = self.structs.borrow().get(&name).unwrap().clone();
        }
        let base_pos = self.stack_pos;
        let mut offset = 0;
        let mut stack_offset = 0;
        for (field_name, field_expr) in fields {
            let field = struct_data.elements.get(field_name).expect("Unknown field");
            let field_type = &field.ty;
            match field_type {
                Type::Struct(_) => {
                    self.stack_pos -= offset;
                    stack_offset += offset;
                }
                _ => {}
            }
            self.eval_expr(field_expr, field_type);
            let sized_reg = self.reg_for_size("rax", field_type).unwrap();
            let size_word = self.get_word(field_type);
            let field_pos = base_pos - offset;
            offset += self.type_size(field_type);
            // can break code
            match field_type {
                Type::Array(..) => {}
                Type::Struct(..) => {}
                _ => {
                    self.emit_func_data(format!(
                        "    mov {} [rbp - {}], {}",
                        size_word, field_pos, sized_reg
                    ));
                }
            }
        }
        self.stack_pos += stack_offset;
        "rax".to_string()
    }

    fn emit_typed_load(&mut self, mem_operand: &str, expected_type: &Type) {
        let size_word = self.get_word(expected_type);
        let sized_rax = self.reg_for_size("rax", expected_type).unwrap();

        match expected_type {
            Type::Primitive(TokenType::I8) | Type::Primitive(TokenType::I16) => {
                self.emit_func_data(format!("    movsx rax, {} {}", size_word, mem_operand));
            }

            Type::Primitive(TokenType::I32) => {
                self.emit_func_data(format!("    movsxd rax, {} {}", size_word, mem_operand));
            }

            Type::Primitive(TokenType::U8) | Type::Primitive(TokenType::U16) => {
                self.emit_func_data(format!("    movzx rax, {} {}", size_word, mem_operand));
            }

            Type::Primitive(TokenType::U32) => {
                self.emit_func_data(format!("    mov eax, {} {}", size_word, mem_operand));
            }

            _ => {
                self.emit_func_data(format!(
                    "    mov {}, {} {}",
                    sized_rax, size_word, mem_operand
                ));
            }
        }
    }

    fn handle_struct_move(
        &mut self,
        expr: &Box<Expr>,
        ty: &Type,
        field_offset: usize,
    ) -> Option<usize> {
        match &expr.ty {
            ExprType::Variable(var_name) => {
                let var = self.lookup_var(var_name);
                Some(var.stack_pos - field_offset)
            }
            ExprType::Deref(inner) => {
                self.eval_expr(inner, &Type::Pointer(Box::new(ty.clone())));
                // rax = pointer value = base address, caller adds field_offset
                if field_offset != 0 {
                    self.emit_func_data(format!("    add rax, {}", field_offset));
                }
                None
            }
            ExprType::StructMember { base, name } => {
                let base_type = base.get_type(self);
                let base_type = self.resolve_generic_inst(&base_type);
                let struct_name = match &base_type {
                    Type::Struct(n) => n.clone(),
                    _ => self::panic!("member access on non-struct"),
                };
                let inner_field = self
                    .structs
                    .borrow()
                    .get(&struct_name)
                    .unwrap()
                    .elements
                    .get(name)
                    .unwrap()
                    .clone();

                match self.handle_struct_move(base, &base_type, inner_field.offset + field_offset) {
                    Some(base_offset) => Some(base_offset),
                    None => None,
                }
            }
            _ => self::panic!("struct member access on a temporary is not supported"),
        }
    }

    fn gen_expr_struct_member(&mut self, base: &Box<Expr>, name: &String) -> String {
        let base_type = base.get_type(self);
        let base_type = self.resolve_generic_inst(&base_type);
        let struct_name = match &base_type {
            Type::Struct(n) => n.clone(),
            _ => self::panic!("member access on non-struct: {:?}", base_type),
        };
        let field = self
            .structs
            .borrow()
            .get(&struct_name)
            .unwrap()
            .elements
            .get(name)
            .unwrap()
            .clone();

        match self.handle_struct_move(base, &base_type, field.offset) {
            Some(static_offset) => match &field.ty {
                Type::Struct(_) | Type::Enum(..) => {
                    self.emit_func_data(format!("    lea rax, [rbp - {}]", static_offset));
                }
                _ => {
                    self.emit_typed_load(&format!("[rbp - {}]", static_offset), &field.ty);
                }
            },
            None => match &field.ty {
                Type::Struct(_) | Type::Enum(..) => {}
                _ => {
                    self.emit_typed_load("[rax]", &field.ty);
                }
            },
        }

        "rax".to_string()
    }

    fn gen_expr_deref(&mut self, expr: &Box<Expr>, expected_type: &Type) -> String {
        self.eval_expr(expr, expected_type);
        self.emit_typed_load("[rax]", expected_type);
        "rax".to_string()
    }

    fn gen_expr_addres_of(&mut self, expr: &Box<Expr>) -> String {
        match &expr.ty {
            ExprType::Variable(name) => {
                let var = self.lookup_var(name);
                if var.global_flag {
                    self.emit_func_data(format!("    lea rax, [rel {}]", name));
                } else {
                    self.emit_func_data(format!("    lea rax, [rbp - {}]", var.stack_pos));
                }
                "rax".to_string()
            }

            ExprType::StructMember { base, name } => {
                self.member_addr(base, name);
                "rax".to_string()
            }

            ExprType::Index { base, index } => {
                let elem_type = expr.get_type(self);
                let elem_size = self.type_size(&elem_type);
                let base_type = base.get_type(self);

                // eval base first, push it
                self.eval_expr(base, &base_type);
                self.push_result();

                // eval index, scale it
                self.eval_expr(index, &Type::Primitive(TokenType::I32));
                self.emit_func_data(format!("    imul rax, rax, {}", elem_size));

                // pop base, add scaled index
                self.pop_into("rbx");
                self.emit_func_data("    add rax, rbx".to_string());
                "rax".to_string()
            }

            ExprType::Deref(inner) => {
                // &*ptr == ptr
                let ptr_type = Type::Pointer(Box::new(expr.get_type(self)));
                self.eval_expr(inner, &ptr_type)
            }

            _ => self::panic!("Cannot take address of this expression: {:?}", expr),
        }
    }

    fn gen_expr_index(
        &mut self,
        base: &Box<Expr>,
        index: &Box<Expr>,
        expected_type: &Type,
    ) -> String {
        let arr_ty = &base.get_type(self);
        self.eval_expr(base, arr_ty);
        self.push_result();
        let index_reg = self.eval_expr(index, &Type::Primitive(TokenType::I64));
        let elem_size = self.type_size(expected_type);
        self.emit_func_data(format!("    imul rax, rax, {}", elem_size));
        self.pop_into("rbx");
        self.emit_func_data(format!("    add rax, rbx"));
        self.emit_typed_load("[rax]", expected_type);
        "rax".to_string()
    }

    fn gen_array_init(&mut self, elements: &Vec<Expr>, expected_type: &Type) -> String {
        let elem_type = match expected_type {
            Type::Array(elem_ty, _) => *elem_ty.clone(),
            _ => self::panic!("gen_array_init called with non-array type"),
        };
        let elem_size = self.type_size(&elem_type);
        let base_pos = self.stack_pos;

        for (i, elem) in elements.iter().enumerate() {
            self.eval_expr(elem, &elem_type);
            let sized_reg = self.reg_for_size("rax", &elem_type).unwrap();
            let size_word = self.get_word(&elem_type);
            let offset = base_pos - (i * elem_size);

            self.emit_func_data(format!(
                "    mov {} [rbp - {}], {}",
                size_word, offset, sized_reg
            ));
        }
        self.emit_func_data(format!("    lea rax, [rbp - {}]", base_pos));
        "rax".to_string()
    }

    fn gen_size_of(&mut self, stmt: &Type) -> String {
        let ty = self.ensure_monomorphized(stmt);
        let size = self.type_size(&ty);
        self.emit_func_data(format!("    mov rax, {}", size));
        "rax".to_string()
    }

    fn gen_string(&mut self, str: &String) -> String {
        let id = self.get_id();
        self.emit_data(format!("str_{}: db \"{}\", 0", id, str));
        self.emit_func_data(format!("    lea rax, [rel str_{}]", id));
        "rax".to_string()
    }

    fn gen_cast(&mut self, expr: &Box<Expr>, target_ty: &Type) -> String {
        let src_ty = expr.get_type(self);
        let src_ty = self.ensure_monomorphized(&src_ty);
        self.eval_expr(expr, &src_ty);

        let src_size = self.type_size(&src_ty);
        let target_size = self.type_size(target_ty);
        if src_size < target_size {
            let src_reg = self.reg_for_size("rax", &src_ty).unwrap();
            let target_reg = self.reg_for_size("rax", target_ty).unwrap();

            let is_unsigned = is_unsigned(&src_ty);

            if is_unsigned {
                if src_size == 4 && target_size == 8 {
                    self.emit_func_data("    mov eax, eax".to_string());
                } else {
                    self.emit_func_data(format!("    movzx {}, {}", target_reg, src_reg));
                }
            } else {
                if src_size == 4 && target_size == 8 {
                    self.emit_func_data("    movsxd rax, eax".to_string());
                } else {
                    if !matches!(src_ty, Type::Array(_, _)) && !matches!(src_ty, Type::Pointer(_)) {
                        self.emit_func_data(format!("    movsx {}, {}", target_reg, src_reg));
                    }
                }
            }
        }

        self.reg_for_size("rax", target_ty).unwrap()
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

    pub fn enum_get_size(&self, base: &String) -> usize {
        let mut size = 0;
        let enum_data = self.enums.borrow().get(base).cloned().unwrap();
        for (name, data) in enum_data.variants.iter() {
            let mut res_size = 0;
            for i in data.args.iter() {
                res_size += self.type_size(&i.ty);
            }
            if res_size > size {
                size = res_size;
            }
        }
        // accounting for tag
        size + TAG_SIZE
    }

    fn handle_generic_enum(
        &mut self,
        enum_data: &EnumData,
        values: &Vec<EnumExprField>,
        variant: &String,
    ) -> String {
        let mut type_map: HashMap<String, Type> = HashMap::new();
        for (name, field) in enum_data.variants.iter() {
            if field.name == *variant {
                for (index, enum_field) in field.args.iter().enumerate() {
                    let expr_ty = values[index].expr.get_type(self);
                    self.resolve_generic(&expr_ty, &enum_field.ty, &mut type_map);
                }
            }
        }

        let type_args: Vec<String> = enum_data
            .generic_type
            .iter()
            .map(|param| type_map.get(param).map(type_name).unwrap_or(param.clone()))
            .collect();
        let name = format!("{}__{}", enum_data.name, type_args.join("_"));
        name
    }

    // TODO: make this bettter
    pub fn gen_get_enum_addr(
        &mut self,
        base: &String,
        value: &Vec<EnumExprField>,
        variant: &String,
    ) -> String {
        let pos = self.stack_pos;
        let mut base = base.clone();
        let enum_data = self
            .enums
            .borrow()
            .get(&base)
            .expect(&format!("no enum with name {}", base))
            .clone();
        if enum_data.generic_type.len() > 0 {
            base = self.handle_generic_enum(&enum_data, value, variant);
        }
        let variant_data = enum_data
            .variants
            .get(variant)
            .expect(&format!("in enum {} no field {}", base, variant));
        // if we have value its creating an object

        if value.is_empty() {
            self.emit_func_data(format!(
                "    mov QWORD [rbp - {}], {}",
                pos, variant_data.tag
            ));
            self.emit_func_data(format!("    lea rax, [rbp - {}]", pos));
            return "rax".to_string();
        }

        self.emit_func_data(format!("    mov rax, {}", variant_data.tag));
        self.emit_func_data(format!("    mov [rbp - {}], rax", pos));

        for (index, var) in variant_data.args.clone().iter().enumerate() {
            let res = &value[index];
            let mut var_ty = var.ty.clone();
            match var.ty {
                Type::GenericType(_) => {
                    var_ty = res.expr.get_type(self);
                }
                _ => {}
            }
            self.eval_expr(&res.expr, &var_ty);
            let reg = self.reg_for_size("rax", &var_ty).unwrap();
            let word = self.get_word(&var_ty);
            match &var_ty {
                Type::Primitive(_) | Type::Array(..) | Type::Pointer(_) => {
                    self.emit_func_data(format!(
                        "    mov {} [rbp - {}], {}",
                        word,
                        self.stack_pos - var.offset,
                        reg
                    ));
                }
                _ => {}
            }
        }
        self.emit_func_data(format!("    lea rax, [rbp - {pos}]"));

        return "rax".to_string();
    }

    pub fn gen_get_enum(
        &mut self,
        base: &String,
        value: &Vec<EnumExprField>,
        variant: &String,
    ) -> String {
        let pos = self.alloc(TAG_SIZE);
        let mut base = base.clone();

        let enum_data = self
            .enums
            .borrow()
            .get(&base)
            .expect(&format!("no enum with name {}", base))
            .clone();

        if enum_data.generic_type.len() > 0 {
            base = self.handle_generic_enum(&enum_data, value, variant);
        }

        let variant_data = enum_data
            .variants
            .get(variant)
            .expect(&format!("in enum {} no field {}", base, variant));

        self.emit_func_data(format!("    mov rax, {}", variant_data.tag));

        if !value.is_empty() {
            self.emit_func_data(format!("    mov [rbp - {}], rax", pos));
            for (index, var) in variant_data.args.clone().iter().enumerate() {
                let res = &value[index];
                let mut var_ty = var.ty.clone();

                match var.ty {
                    Type::GenericType(_) => {
                        var_ty = res.expr.get_type(self);
                    }
                    _ => {}
                }

                self.eval_expr(&res.expr, &var_ty);

                let reg = self.reg_for_size("rax", &var_ty).unwrap();
                let word = self.get_word(&var_ty);

                match &var_ty {
                    Type::Primitive(_) | Type::Array(..) | Type::Pointer(_) => {
                        self.emit_func_data(format!(
                            "    mov {} [rbp - {}], {}",
                            word,
                            pos - var.offset,
                            reg
                        ));
                    }
                    _ => {
                        self::panic!("gen_get_enum error");
                    }
                }
            }
            self.emit_func_data(format!("    lea rax, [rbp - {}]", pos));
        }

        return "rax".to_string();
    }

    pub fn eval_expr(&mut self, expr: &Expr, expected_type: &Type) -> String {
        match &expr.ty {
            ExprType::ArrayInit { elements } => self.gen_array_init(elements, expected_type),
            ExprType::Number(num) => self.gen_expr_num(num, expected_type),

            ExprType::Variable(var) => self.gen_expr_var(var, expected_type),

            ExprType::Binary { op, left, right } => {
                let expr_ty = expr.get_type(self);
                self.gen_expr_binary((op, left, right), &expr_ty)
            }

            ExprType::Unary { op, expr: inner } => self.gen_expr_unary(op, inner, expected_type),

            ExprType::Call {
                name,
                args,
                generics,
            } => {
                let (func_data, overload_pos) = self.resolve_call(name, args, generics).unwrap();
                self.gen_call(&name, args, &func_data, overload_pos, generics)
            }

            ExprType::Deref(inner) => {
                let ty = expr.get_type(self);
                self.gen_expr_deref(inner, &ty)
            }

            ExprType::Index { base, index } => {
                let ty = expr.get_type(self);
                let ty = self.ensure_monomorphized(&ty);
                self.gen_expr_index(base, index, &ty)
            }

            ExprType::StructMember { base, name } => {
                let ty = expr.get_type(self);
                self.gen_expr_struct_member(base, name)
            }

            ExprType::Cast { expr, ty } => self.gen_cast(expr, ty),

            ExprType::StructInit {
                fields,
                struct_name_ty,
            } => self.gen_expr_struct_init(fields, struct_name_ty),

            ExprType::SizeOf { ty } => self.gen_size_of(ty),
            ExprType::Float(_) => self::panic!("floats not implemented"),
            ExprType::String { str } => self.gen_string(str),
            ExprType::GetEnum {
                base,
                value,
                variant,
            } => self.gen_get_enum(base, value, variant),
        }
    }
}
