use std::fmt::format;

use crate::Ir::expr::{self, BinOp, EnumExprField, Expr, Lookup, UnaryOp};
use crate::Ir::stmt::{Declaration, EnumVariant, StructField};
use crate::sem_analysis::{check_types, coerce_numeric, is_numeric};

use super::*;
use super::{get_word, reg_for_size};

impl Lookup for Gen {
    fn look_var(&self, name: &String) -> Type {
        if self.structs.get(name).is_some() {
            return Type::Struct(name.clone());
        } else {
            let var = self.lookup_var(name);
            var.var_type.clone()
        }
    }
    fn look_unary(&self, op: &UnaryOp, expr: &Box<Expr>) -> Type {
        match op {
            UnaryOp::Neg => expr.get_type(self),
            UnaryOp::Not => Type::Primitive(TokenType::CharType), // boolean
        }
    }
    fn look_binary(&self, op: &BinOp, left: &Box<Expr>, right: &Box<Expr>) -> Type {
        let lty = left.get_type(self);
        let rty = right.get_type(self);
        coerce_numeric(&lty, &rty)
    }
    fn look_struct_init(&self, struct_name: &String) -> Type {
        if let Some(_struct_data) = self.structs.get(struct_name) {
            Type::Struct(struct_name.clone())
        } else {
            self::panic!("Struct {} not found in get_type", struct_name);
        }
    }
    fn look_deref(&self, ptr_expr: &Box<Expr>) -> Type {
        match ptr_expr.get_type(self) {
            Type::Pointer(inner) => *inner,
            _ => self::panic!("Cannot dereference a non-pointer: {:?}",ptr_expr.get_type(self)),
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
        let struct_name = match &base_ty {
            Type::Struct(n) => n.clone(),
            Type::Pointer(inner) => match inner.as_ref() {
                Type::Struct(n) => n.clone(),
                _ => self::panic!("pointer to non-struct"),
            },
            _ => self::panic!("member access on non-struct"),
        };
        let struct_data = self.structs.get(&struct_name).unwrap();
        let field = struct_data.elements.get(name).unwrap();
        field.ty.clone()
    }
    fn look_call(&self, name: &String, args: &Vec<Expr>) -> Type {
        let vec_func_data = self.functions.get(name).unwrap();
        let func_data = vec_func_data
            .iter()
            .find(|func| {
                if func.args.len() != args.len() {
                    return false;
                }
                args.iter()
                    .enumerate()
                    .all(|(index, expr)| check_types(&expr.get_type(self), &func.args[index].ty))
            })
            .expect(&format!("no matching overload for function '{}'", name));
        
        func_data.return_type.clone()
    }
    fn look_array_init(&self, elements: &Vec<Expr>) -> Type {
        if elements.len() > 0 {
            return elements[0].get_type(self);
        } else {
            Type::Unknown
        }
    }

    fn look_get_enum(&self, base: &String) -> Type {
        Type::Enum(base.clone())
    }
}

impl Expr {
    /// Returns the Type of this expression
    pub fn get_type(&self, helper: &impl Lookup) -> Type {
        match self {
            Expr::Number(_) => Type::Primitive(TokenType::LongType),
            Expr::Float(_) => todo!(),
            Expr::Variable(var_name) => helper.look_var(var_name),
            Expr::Binary { op, left, right } => helper.look_binary(op, left, right),
            Expr::Unary { op, expr } => helper.look_unary(op, expr),
            Expr::Call {
                name,
                args,
                generics,
            } => helper.look_call(name, args),
            Expr::StructInit {
                struct_name_ty,
                fields,
            } => helper.look_struct_init(struct_name_ty),
            Expr::StructMember { base, name } => helper.look_struct_member(base, name),
            Expr::Deref(expr) => helper.look_deref(expr),
            Expr::AddressOf(expr) => helper.look_addres_of(expr),
            Expr::Index { base, index } => helper.look_index(base, index),
            Expr::ArrayInit { elements } => helper.look_array_init(elements),
            Expr::SizeOf { ty } => Type::Primitive(TokenType::LongType),
            Expr::String { str } => {
                return Type::Array(
                    Box::new(Type::Primitive(TokenType::CharType)),
                    str.len() + 1,
                );
            }
            Expr::GetEnum {
                base,
                variant,
                value,
            } => helper.look_get_enum(base),
            Expr::Cast { expr, ty } => ty.clone(),
        }
    }
}

impl Gen {
    fn gen_expr_binop(
        &mut self,
        op: &BinOp,
        left_reg: &str,  // rbx/ebx etc
        right_reg: &str, // rax/eax etc
        expected_type: &Type,
    ) {
        let result_reg = reg_for_size("rax", expected_type).unwrap();
        let left_sized = reg_for_size("rbx", expected_type).unwrap();

        match op {
            BinOp::Add => {
                self.emit_func_data(format!("    add {}, {}", left_reg, right_reg));
                self.emit_func_data(format!("    mov {}, {}", result_reg, left_sized));
            }
            BinOp::Sub => {
                self.emit_func_data(format!("    sub {}, {}", left_reg, right_reg));
                self.emit_func_data(format!("    mov {}, {}", result_reg, left_sized));
            }
            BinOp::Mul => {
                self.emit_func_data(format!("    imul {}, {}", left_reg, right_reg));
                self.emit_func_data(format!("    mov {}, {}", result_reg, left_sized));
            }
            BinOp::Div => {
                if self.type_size(expected_type) == 8 {
                    self.emit_func_data("    cqo".to_string());
                } else {
                    self.emit_func_data("    cdq".to_string());
                }
                self.emit_func_data(format!(
                    "    idiv {}",
                    reg_for_size("rbx", expected_type).unwrap()
                ));
                // result already in rax
            }
            BinOp::Mod => {
                if self.type_size(expected_type) == 8 {
                    self.emit_func_data("    cqo".to_string());
                } else {
                    self.emit_func_data("    cdq".to_string());
                }
                self.emit_func_data(format!(
                    "    idiv {}",
                    reg_for_size("rbx", expected_type).unwrap()
                ));
                // remainder in rdx, move to rax
                self.emit_func_data(format!(
                    "    mov {}, {}",
                    result_reg,
                    reg_for_size("rdx", expected_type).unwrap()
                ));
            }
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                self.emit_func_data(format!("    cmp {}, {}", right_reg, left_reg));
                let set_instr = match op {
                    BinOp::Eq => "sete",
                    BinOp::Neq => "setne",
                    BinOp::Lt => "setl",
                    BinOp::Lte => "setle",
                    BinOp::Gt => "setg",
                    BinOp::Gte => "setge",
                    _ => unreachable!(),
                };
                self.emit_func_data(format!("    {} al", set_instr));
                self.emit_func_data(format!("    movzx {}, al", result_reg));
            }
            BinOp::And => {
                self.emit_func_data(format!("    cmp {}, 0", left_reg));
                self.emit_func_data("    setne al".to_string());
                self.emit_func_data(format!("    cmp {}, 0", right_reg));
                self.emit_func_data("    setne dl".to_string());
                self.emit_func_data("    and al, dl".to_string());
                self.emit_func_data(format!("    movzx {}, al", result_reg));
            }
            BinOp::Or => {
                self.emit_func_data(format!("    cmp {}, 0", left_reg));
                self.emit_func_data("    setne al".to_string());
                self.emit_func_data(format!("    cmp {}, 0", right_reg));
                self.emit_func_data("    setne dl".to_string());
                self.emit_func_data("    or al, dl".to_string());
                self.emit_func_data(format!("    movzx {}, al", result_reg));
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
            Type::GenericType(name) => self.generics.get(name).unwrap(),
            _ => expected_type,
        };
        let sized_rax = reg_for_size("rax", expected_type).unwrap();
        self.emit_func_data(format!("    mov {}, {}", sized_rax, num));
        "rax".to_string()
    }

    fn gen_expr_var(&mut self, var_name: &String, expected_type: &Type) -> String {
        let var_data = self.lookup_var(var_name).clone();

        if var_data.global_flag {
            match var_data.var_type {
                Type::Primitive(_) | Type::Pointer(_) => {
                    let sized_rax = reg_for_size("rax", &var_data.var_type).unwrap();
                    self.emit_func_data(format!("    mov {}, [rel {}]", sized_rax, var_name));
                }
                _ => {
                    // struct/array — load address
                    self.emit_func_data(format!("    lea rax, [rel {}]", var_name));
                }
            }
            return "rax".to_string();
        }

        match &var_data.var_type {
            Type::Primitive(_) => {
                let actual_size = self.type_size(&var_data.var_type);
                let expected_size = self.type_size(expected_type);
                if expected_size > actual_size {
                    let src_word = get_word(&var_data.var_type);
                    self.emit_func_data(format!(
                        "    movsx rax, {} [rbp - {}]",
                        src_word, var_data.stack_pos
                    ));
                } else {
                    let sized_rax = reg_for_size("rax", &var_data.var_type).unwrap();
                    self.emit_func_data(format!(
                        "    mov {}, {} [rbp - {}]",
                        sized_rax,
                        get_word(&var_data.var_type),
                        var_data.stack_pos
                    ));
                }
            }
            Type::Pointer(_) => {
                self.emit_func_data(format!("    mov rax, [rbp - {}]", var_data.stack_pos));
            }
            Type::Array(ty, size) => {
                let src = var_data.stack_pos;
                let dst = self.stack_pos;
                let id = self.get_id();
                let res_size = self.type_size(ty);
                let reg = reg_for_size("rax", ty).unwrap();
                self.emit_func_data(format!("    lea rsi, [rbp - {}]", src));
                self.emit_func_data(format!("    lea rdi, [rbp - {}]", dst));
                self.emit_func_data(format!("    mov rcx, {}", size));
                self.emit_func_data(format!(".copy_loop_{}:", id));
                self.emit_func_data(format!("    mov {reg}, [rsi]"));
                self.emit_func_data(format!("    mov [rdi], {reg}"));
                self.emit_func_data(format!("    add rsi, {res_size}"));
                self.emit_func_data(format!("    add rdi, {res_size}"));
                self.emit_func_data("    dec rcx".to_string());
                self.emit_func_data(format!("    jnz .copy_loop_{}", id));
            }
            Type::Enum(ty) => {
                self.emit_func_data(format!("    mov rax, [rbp - {}]", var_data.stack_pos));
            }
            _ => {
                // struct/array — load address
                self.emit_func_data(format!("    lea rax, [rbp - {}]", var_data.stack_pos));
            }
        }
        "rax".to_string()
    }

    fn gen_expr_binary(
        &mut self,
        data: (&BinOp, &Box<Expr>, &Box<Expr>),
        expected_type: &Type,
    ) -> String {
        let (op, left, right) = data;
        self.eval_expr(right, expected_type);
        self.push_result();
        self.eval_expr(left, expected_type);
        self.pop_into("rbx");

        let left_reg = reg_for_size("rbx", &expected_type).unwrap(); // e.g. ebx
        let right_reg = reg_for_size("rax", &expected_type).unwrap();

        self.gen_expr_binop(op, &left_reg, &right_reg, expected_type);

        left_reg
    }

    fn gen_expr_unary(&mut self, op: &UnaryOp, expr: &Box<Expr>, expected_type: &Type) -> String {
        match op {
            UnaryOp::Neg => {
                self.eval_expr(expr, expected_type);
                let sized = reg_for_size("rax", expected_type).unwrap();
                self.emit_func_data(format!("    neg {}", sized));
            }
            UnaryOp::Not => {
                self.eval_expr(expr, expected_type);
                let sized = reg_for_size("rax", expected_type).unwrap();
                self.emit_func_data(format!("    cmp {}, 0", sized));
                self.emit_func_data("    sete al".to_string());
                self.emit_func_data(format!("    movzx {}, al", sized));
            }
        }
        "rax".to_string()
    }

    pub fn transform_generic_name(&self, name: &String, generics: &Vec<Type>) -> String {
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
    

    fn convert_generic_arg(&self, arg: &Declaration, arg_ty: &Type, generics: &Vec<Type>, index: usize) -> Declaration {
        match arg_ty{
            Type::GenericInst(name, _) => {
                let normal_name = self.transform_generic_name(name, generics);
                let ty = if self.structs.contains_key(&normal_name) {
                    Type::Struct(normal_name)
                } else if self.enums.contains_key(&normal_name) {
                    Type::Enum(normal_name)
                } else {
                    generics[index].clone()
                };
                Declaration {
                    name: arg.name.clone(),
                    ty,
                    initializer: arg.initializer.clone(),
                }
            }
            Type::Pointer(ty) => {
                let mut decl = self.convert_generic_arg(arg, ty, generics, index);
                decl.ty = Type::Pointer(Box::new(decl.ty));
                decl
            }
            Type::Array(ty, size) => {
                let mut decl = self.convert_generic_arg(arg, ty, generics, index);
                decl.ty = Type::Array(Box::new(decl.ty), *size);
                decl
            }
            _ => arg.clone(),
        }
    }

    fn convert_generic_args(&self, args: &Vec<Declaration>, generics: &Vec<Type>) -> Vec<Declaration> {
        args.iter()
            .enumerate()
            .map(|(index, arg)| self.convert_generic_arg(arg, &arg.ty , generics, index))
            .collect()
    }


    fn gen_expr_call(
        &mut self,
        name: &String,
        args: &Vec<Expr>,
        func_data: &FuncData,
        overload_pos: usize,
        generics: &Vec<Type>,
    ) -> String {
        let mut name = name.clone();
        let stack_pos_save = self.stack_pos;
        self.stack_pos = 0;
        for (index, arg) in args.iter().enumerate() {
            let arg_type = func_data.args[index].ty.clone();
            self.eval_expr(arg, &arg_type);
            self.emit_func_data("    push rax".to_string());
        }
        for (index, generic) in func_data.generic.iter().enumerate() {
            self.generics
            .insert(generic.clone(), generics[index].clone());
        }
        
        if func_data.generic.len() > 0 {
            let generic_data = self.generic_func.get(&name).unwrap().clone();
            let mangled = self.transform_generic_name(&name, generics);
            let new_args = self.convert_generic_args(&func_data.args, generics);

            let res_func_data = FuncData {
                args: new_args.clone(),
                generic: Vec::new(),
                return_type: func_data.return_type.clone(),
            };
            self.functions.insert(mangled.clone(), vec![res_func_data]);
            name = self.transform_generic_name(&name, generics);
            match generic_data {
                Stmt::InitFunc {
                    name,
                    generic_types,
                    args,
                    ret_type,
                    data,
                } => {
                    self.gen_func((&mangled, &new_args, &ret_type, &data, &Vec::new()));
                }
                _ => self::panic!("error"),
            };
        }
        // pop into arg registers in reverse order
        for (index, _) in args.iter().enumerate().rev() {
            let mut arg_type = func_data.args[index].ty.clone();
            match &arg_type {
                Type::GenericInst(name, grg) => {
                    let mangled = self.transform_generic_name(name, generics);
                    if self.structs.get(&mangled).is_some() {
                        arg_type = Type::Struct(mangled);
                    } else if self.enums.get(&mangled).is_some() {
                        arg_type = Type::Enum(mangled);
                    }
                }
                _ => {},
            }
            let arg_reg = arg_pos(index, &arg_type);
            self.emit_func_data(format!("    pop {}", to_base_reg(&arg_reg)));
            // then size it down if needed
            reg_for_size(&to_base_reg(&arg_reg), &arg_type).unwrap();
        }
        if self.functions.get(&name).unwrap().len() > 1 {
            self.emit_func_data(format!("    call {}___{}", name, overload_pos));
        } else {
            self.emit_func_data(format!("    call {}", name));
        }
        self.stack_pos = stack_pos_save;
        return "rax".to_string();
    }

    fn generic_to_ty(&self, ty: &Type, type_map: &HashMap<String, Type>) -> Type {
        match ty {
            Type::GenericType(name) => {
                return type_map.get(name).cloned().unwrap();
            }
            Type::Array(arr_ty, size) => {
                let res = self.generic_to_ty(arr_ty, type_map);
                return res;
            }
            Type::Pointer(ptr_ty) => {
                let res = self.generic_to_ty(ptr_ty, type_map);
                return res;
            }
            _ => return ty.clone(),
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
        let struct_data = self.structs.get(struct_name).unwrap().clone();

        let mut type_map: HashMap<String, Type> = HashMap::new();
        for (index, (_, field)) in struct_data.elements.iter().enumerate() {
            let expr_ty = fields[index].1.get_type(self);
            self.resolve_generic(&expr_ty, &field.ty, &mut type_map);
        }

        let mut new_elements = HashMap::new();
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

        // Register if not already monomorphized
        if !self.structs.contains_key(&name) {
            let new_data = StructData {
                name: name.clone(),
                generic_type: Vec::new(),
                elements: new_elements,
                byte_size: offset,
            };
            self.structs.insert(name.clone(), new_data);
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
            .get(struct_name)
            .expect("Unknown struct")
            .clone();
        if struct_data.generic_type.len() > 0 {
            let name = self.gen_generic_struct(fields, struct_name);
            struct_data = self.structs.get(&name).unwrap().clone();
        }
        let base_pos = self.stack_pos;
        for (field_name, field_expr) in fields {
            let field = struct_data.elements.get(field_name).expect("Unknown field");
            let field_type = &field.ty;
            self.eval_expr(field_expr, field_type);
            let sized_reg = reg_for_size("rax", field_type).unwrap();
            let size_word = get_word(field_type);
            let field_pos = base_pos - field.offset;
            //match field_type {
            //    Type::Array(.. ) => {},
            //    _ => {
            //        self.emit_func_data(format!(
            //            "    mov {} [rbp - {}], {}",
            //            size_word, field_pos, sized_reg
            //        ));
            //    }
            //}
        }
        "rax".to_string()
    }

    fn gen_expr_struct_member(&mut self, base: &Box<Expr>, name: &String) -> String {
        let base_type = base.get_type(self);

        let struct_name = match &base_type {
            Type::Struct(n) => n.clone(),
            _ => self::panic!("member access on non-struct"),
        };

        let field = self
            .structs
            .get(&struct_name)
            .unwrap()
            .elements
            .get(name)
            .unwrap()
            .clone();
        let size_word = get_word(&field.ty);

        match base.as_ref() {
            Expr::Deref(inner) => {
                // -> operator: eval inner to get pointer value, add offset, read
                self.eval_expr(inner, &Type::Pointer(Box::new(base_type.clone())));
                self.emit_func_data(format!("    add rax, {}", field.offset));
                let reg = reg_for_size("rax", &field.ty).unwrap();
                self.emit_func_data(format!("    mov {}, {} [rax]", reg, size_word));
            }
            Expr::Variable(var_name) => {
                // . operator: compile-time offset
                let var = self.lookup_var(var_name);
                let reg = reg_for_size("rax", &field.ty).unwrap();
                match var.var_type {
                    Type::Primitive(..) => {
                        let field_addr = var.stack_pos - field.offset;
                        self.emit_func_data(format!(
                            "    mov {}, {} [rbp - {}]",
                            reg, size_word, field_addr
                        ));
                    }
                    _ => {
                        let field_addr = var.stack_pos;
                        self.emit_func_data(format!(
                            "    lea {}, {} [rbp - {}]",
                            reg, size_word, field_addr
                        ));
                    }
                }
            }
            _ => {
                // chained a.b.c — runtime fallback
                self.eval_expr(base, &base_type);
                self.emit_func_data(format!("    add rax, {}", field.offset));
                let reg = reg_for_size("rax", &field.ty).unwrap();
                self.emit_func_data(format!("    mov {}, {} [rax]", reg, size_word));
            }
        }
        "rax".to_string()
    }

    fn gen_expr_deref(&mut self, expr: &Box<Expr>, expected_type: &Type) -> String {
        self.eval_expr(expr, expected_type);
        let size_word = get_word(expected_type);
        let sized_rax = reg_for_size("rax", expected_type).unwrap();

        match expected_type {
            Type::Primitive(TokenType::IntType)
            | Type::Primitive(TokenType::ShortType)
            | Type::Primitive(TokenType::CharType) => {
                self.emit_func_data(format!("    movsx rax, {} [rax]", size_word));
            }
            _ => {
                self.emit_func_data(format!("    mov {}, {} [rax]", sized_rax, size_word));
            }
        }
        "rax".to_string()
    }

    fn gen_expr_addres_of(&mut self, expr: &Box<Expr>) -> String {
        match &**expr {
            Expr::Variable(name) => {
                let var = self.lookup_var(name);
                if var.global_flag {
                    self.emit_func_data(format!("    lea rax, [rel {}]", name));
                } else {
                    self.emit_func_data(format!("    lea rax, [rbp - {}]", var.stack_pos));
                }
                "rax".to_string()
            }

            Expr::StructMember { base, name } => {
                self.member_addr(base, name);
                "rax".to_string()
            }

            Expr::Index { base, index } => {
                let elem_type = expr.get_type(self);
                let elem_size = self.type_size(&elem_type);
                let base_type = base.get_type(self);

                // eval base first, push it
                self.eval_expr(base, &base_type);
                self.push_result();

                // eval index, scale it
                self.eval_expr(index, &Type::Primitive(TokenType::LongType));
                self.emit_func_data(format!("    imul rax, rax, {}", elem_size));

                // pop base, add scaled index
                self.pop_into("rbx");
                self.emit_func_data("    add rax, rbx".to_string());
                "rax".to_string()
            }

            Expr::Deref(inner) => {
                // &*ptr == ptr
                let ptr_type = Type::Pointer(Box::new(expr.get_type(self)));
                self.eval_expr(inner, &ptr_type)
            }

            _ => self::panic!("Cannot take address of this expression"),
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
        self.eval_expr(index, &Type::Primitive(TokenType::LongType));
        //runtime checking
        match arr_ty {
            Type::Array(ty, size) => {
                self.emit_func_data(format!("    cmp rax, {}", size));
                self.emit_func_data(format!("    jge __bounds_fail__"));
                self.emit_func_data(format!("    cmp rax, 0"));
                self.emit_func_data(format!("    jl __bounds_fail__"));
            }
            _ => {}
        }

        let elem_size = self.type_size(expected_type);
        self.emit_func_data(format!("    imul rax, rax, {}", elem_size,));
        self.pop_into("rbx");
        self.emit_func_data(format!("    add rax, rbx"));
        let size_word = get_word(&expected_type);
        match &expected_type {
            Type::Primitive(TokenType::CharType)
            | Type::Primitive(TokenType::ShortType)
            | Type::Primitive(TokenType::IntType) => {
                self.emit_func_data(format!("    movsx rax, {} [rax]", size_word));
            }
            _ => {
                self.emit_func_data(format!("    mov rax, {} [rax]", size_word));
            }
        }
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
            let sized_reg = reg_for_size("rax", &elem_type).unwrap();
            let size_word = get_word(&elem_type);
            let offset = base_pos - (i * elem_size);

            self.emit_func_data(format!(
                "    mov {} [rbp - {}], {}",
                size_word, offset, sized_reg
            ));
        }
        self.emit_func_data(format!("    lea rax, [rbp - {}]", base_pos));
        "rax".to_string()
    }

    fn gen_size_of(&mut self, stmt: &Box<Stmt>) -> String {
        let ty = {
            match *stmt.clone() {
                Stmt::Declaration(decl) => decl.ty,
                _ => self::panic!("bug"),
            }
        };
        let ty = self.ensure_monomorphized(&ty);
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

    fn gen_cast(&mut self, expr: &Box<Expr>, ty: &Type) -> String {
        self.eval_expr(expr, ty);
        let sized = reg_for_size("rax", ty).unwrap();
        if sized != "rax" {
            self.emit_func_data(format!("    mosvx rax, {}", sized));
        }
        "rax".to_string()
    }

    pub fn enum_get_size(&self, base: &String) -> usize {
        let mut size = 0;
        let enum_data = self.enums.get(base).unwrap();
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
        size + 8
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

    pub fn gen_get_enum(
        &mut self,
        base: &String,
        value: &Vec<EnumExprField>,
        variant: &String,
    ) -> String {
        let pos = self.stack_pos;
        let mut base = base.clone();
        let enum_data = self
            .enums
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
        if !value.is_empty() {
            self.emit_func_data(format!("    mov rax, {}", variant_data.tag));
            self.emit_func_data(format!("    mov [rbp - {}], rax", pos));
            // this reserves space for tag
            self.stack_pos -= 8;
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
                let reg = reg_for_size("rax", &var_ty).unwrap();
                let word = get_word(&var_ty);
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
            self.emit_func_data(format!("    mov rax, [rbp - {pos}]"));
            // returns space
            self.stack_pos += 8;

            return "rax".to_string();
        } else {
            self.emit_func_data(format!("    mov rax, {}", variant_data.tag));
            return "rax".to_string();
        }
    }



    pub fn eval_expr(&mut self, expr: &Expr, expected_type: &Type) -> String {
        match expr {
            Expr::ArrayInit { elements } => self.gen_array_init(elements, expected_type),
            Expr::Number(num) => self.gen_expr_num(num, expected_type),

            Expr::Variable(var) => self.gen_expr_var(var, expected_type),

            Expr::Binary { op, left, right } => {
                let expr_ty = expr.get_type(self);
                self.gen_expr_binary((op, left, right), &expr_ty)
            }

            Expr::Unary { op, expr: inner } => self.gen_expr_unary(op, inner, expected_type),

            Expr::Call {
                name,
                args,
                generics,
            } => {
                if generics.len() > 0 {
                    let vec_func_data = self.functions.get(name).unwrap().clone();
                    return self.gen_expr_call(&name, args, &vec_func_data[0], 0, generics);

                }
                let vec_func_data = self.functions.get(name).unwrap().clone();
                let (overload_pos, func_data) = vec_func_data
                    .iter()
                    .enumerate()
                    .find(|(_, func)| {
                        if func.args.len() != args.len() {
                            return false;
                        }
                        args.iter().enumerate().all(|(i, expr)| {
                            let expr_ty = expr.get_type(self);
                            let param_ty = &func.args[i].ty;
                            check_types(&expr_ty, param_ty)
                        })
                    })
                    .expect(&format!(
                        "no matching overload for function '{}'\ngot: {:?}",
                        name,
                        expr.get_type(self)
                    ));
                self.gen_expr_call(&name, args, &func_data, overload_pos, &Vec::new())
            }

            Expr::Deref(inner) => {
                let ty = expr.get_type(self);
                self.gen_expr_deref(inner, &ty)
            }

            Expr::AddressOf(inner) => self.gen_expr_addres_of(inner),

            Expr::Index { base, index } => {
                let ty = expr.get_type(self);
                self.gen_expr_index(base, index, &ty)
            }

            Expr::StructMember { base, name } => {
                let ty = expr.get_type(self);
                self.gen_expr_struct_member(base, name)
            }

            Expr::Cast { expr, ty } => self.gen_cast(expr, ty),

            Expr::StructInit {
                fields,
                struct_name_ty,
            } => self.gen_expr_struct_init(fields, struct_name_ty),

            Expr::SizeOf { ty } => self.gen_size_of(ty),
            Expr::Float(_) => self::panic!("floats not implemented"),
            Expr::String { str } => self.gen_string(str),
            Expr::GetEnum {
                base,
                value,
                variant,
            } => self.gen_get_enum(base, value, variant),
        }
    }
}
