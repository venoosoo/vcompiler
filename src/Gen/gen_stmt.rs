use std::alloc::Layout;
use std::env::{self, var};
use std::fmt::format;
use std::fs::File;
use std::io::Read;

use super::*;

use crate::Ir::expr::{Expr, Lookup};
use crate::Ir::stmt::{EnumVariant, LValue, MatchField, MatchLeftValue, StructDef, StructField};
use crate::Ir::{Stmt, stmt::Declaration};

fn substitute_type(ty: &Type, params: &Vec<String>, args: &Vec<Type>) -> Type {
    match ty {
        Type::GenericType(name) => {
            if let Some(pos) = params.iter().position(|p| p == name) {
                args[pos].clone()
            } else {
                ty.clone()
            }
        }
        Type::Pointer(inner) => Type::Pointer(Box::new(substitute_type(inner, params, args))),
        Type::Array(inner, size) => {
            Type::Array(Box::new(substitute_type(inner, params, args)), *size)
        }
        Type::GenericInst(name, inner_args) => {
            // substitute inside nested generics e.g. Option<Vec<T>>
            let new_args = inner_args
                .iter()
                .map(|a| substitute_type(a, params, args))
                .collect();
            Type::GenericInst(name.clone(), new_args)
        }
        _ => ty.clone(),
    }
}

impl Gen {
    fn gen_block(&mut self, data: &Vec<Stmt>) {
        self.scopes.push(HashMap::new());
        let temp_stack_pos = self.stack_pos;
        for i in data {
            self.gen_stmt(&i);
        }
        self.scopes.pop();
        self.stack_pos = temp_stack_pos;
    }

    fn monomorphize_struct(&mut self, def: &StructData, type_args: &Vec<Type>) -> Type {
        let mangled = format!(
            "{}__{}",
            def.name,
            type_args
                .iter()
                .map(|t| type_name(t))
                .collect::<Vec<_>>()
                .join("_")
        );
        if self.structs.contains_key(&mangled) {
            return Type::Struct(mangled.clone()); // already done
        }

        // substitute types in fields and recompute offsets
        let mut offset = 0;
        let fields: Vec<StructField> = def
            .elements
            .iter()
            .map(|f| {
                let concrete_ty = substitute_type(&f.1.ty, &def.generic_type, type_args);
                let field_size = self.type_size(&concrete_ty);
                let field = StructField {
                    name: f.1.name.clone(),
                    ty: concrete_ty,
                    offset,
                };
                offset += field_size;
                field
            })
            .collect();
        self.structs.insert(
            mangled.clone(),
            StructData {
                generic_type: Vec::new(),
                name: mangled.clone(),
                elements: fields.iter().map(|f| (f.name.clone(), f.clone())).collect(),
                byte_size: offset, // total size
            },
        );
        return Type::Struct(mangled);
    }

    fn monomorphize_enum(&mut self, def: &EnumData, type_args: &Vec<Type>) -> Type {
        let mangled = format!(
            "{}__{}",
            def.name,
            type_args
                .iter()
                .map(|t| type_name(t))
                .collect::<Vec<_>>()
                .join("_")
        );

        if self.enums.contains_key(&mangled) {
            return Type::Enum(mangled.clone()); // already done
        }

        let mut new_variants = HashMap::new();
        for (var_name, variant) in def.variants.iter() {
            let new_args: Vec<StructField> = variant
                .args
                .iter()
                .map(|arg| StructField {
                    name: arg.name.clone(),
                    ty: substitute_type(&arg.ty, &def.generic_type, type_args),
                    // the tag offest
                    offset: arg.offset + 8,
                })
                .collect();
            new_variants.insert(
                var_name.clone(),
                EnumVariant {
                    name: variant.name.clone(),
                    tag: variant.tag,
                    args: new_args,
                },
            );
        }
        self.enums.insert(
            mangled.clone(),
            EnumData {
                name: mangled.clone(),
                generic_type: Vec::new(),
                variants: new_variants,
            },
        );
        return Type::Enum(mangled);
    }

    pub fn ensure_monomorphized(&mut self, ty: &Type) -> Type {
        match ty {
            Type::GenericInst(name, type_args) => {
                let mangled = type_name(ty);
                // already done?
                if self.structs.contains_key(&mangled) {
                    return Type::Struct(mangled.clone());
                }
                if self.enums.contains_key(&mangled) {
                    return Type::Enum(mangled.clone());
                }
                // find the generic definition and monomorphize
                if let Some(struct_def) = self.structs.get(name).cloned() {
                    return self.monomorphize_struct(&struct_def, type_args);
                } else if let Some(enum_def) = self.enums.get(name).cloned() {
                    return self.monomorphize_enum(&enum_def, type_args);
                } else {
                    self::panic!("unknown generic type: {}", name);
                }
            }
            Type::Pointer(inner) => {
                let ty = self.ensure_monomorphized(inner);
                Type::Pointer(Box::new(ty))
            }
            Type::Array(inner, size) => {
                let ty = self.ensure_monomorphized(inner);
                Type::Array(Box::new(ty), *size)
            }
            _ => ty.clone(),
        }
    }

    fn gen_declaration(&mut self, data: &Declaration) {
        let data_ty = self.ensure_monomorphized(&data.ty);
        let data_ty = match data_ty {
            Type::GenericType(name) => self.generics.get(&name).unwrap().clone(),
            _ => data_ty,
        };

        let stack_pos = self.alloc_type(&data_ty);
        let current_scope = self.scopes.last_mut().unwrap();
        if current_scope.contains_key(&data.name) {
            self::panic!("Variable already declared in this scope");
        }

        let var_data = VarData {
            global_flag: false,
            stack_pos,
            var_type: data_ty.clone(),
        };
        current_scope.insert(data.name.clone(), var_data);

        if let Some(expr) = &data.initializer {
            self.eval_expr(expr, &data_ty);
            match data_ty.clone() {
                Type::Primitive(_) | Type::Pointer(_) | Type::Struct(_) => {
                    let size_word = get_word(&data_ty);
                    let sized_reg = reg_for_size("rax", &data_ty).unwrap();
                    self.emit_func_data(format!(
                        "    mov {} [rbp - {}], {}",
                        size_word, stack_pos, sized_reg
                    ));
                }
                Type::Array(ref ty, size) => match **ty {
                    Type::Primitive(TokenType::CharType) => {
                        let size_word = get_word(&data_ty);
                        let sized_reg = reg_for_size("rax", &data_ty).unwrap();
                        self.emit_func_data(format!(
                            "    mov {} [rbp - {}], {}",
                            size_word, stack_pos, sized_reg
                        ));
                    }
                    _ => {}
                },
                Type::Enum(_) => {
                    match expr {
                        Expr::GetEnum { base, variant, value } => {
                            if value.len() == 0 {
                                let size_word = get_word(&data_ty);
                                let sized_reg = reg_for_size("rax", &data_ty).unwrap();
                                self.emit_func_data(format!(
                                    "    mov {} [rbp - {}], {}",
                                    size_word, stack_pos, sized_reg
                                ));
                            }
                        }
                        _ => {},
                    }
                }
                _ => {} // structs/arrays already written to stack by their eval_expr
            }
        }
    }

    pub fn calc_lvalue(&mut self, target: &LValue) -> (Addr, Type) {
        match target {
            LValue::Variable(name) => {
                let var = self.lookup_var(name);
                if !var.global_flag {
                    (Addr::Stack(var.stack_pos as isize), var.var_type.clone())
                } else {
                    (Addr::Reg(format!("rel {}", name)), var.var_type.clone())
                }
            }

            LValue::Field { base, name } => {
                let (addr, ty) = self.calc_lvalue(base);
                match ty {
                    Type::Pointer(inner) => match inner.as_ref() {
                        Type::Struct(struct_name) => {
                            // load pointer value into rsi, then deref
                            match &addr {
                                Addr::Stack(pos) => {
                                    self.emit_func_data(format!("    mov rsi, [rbp - {}]", pos));
                                }
                                Addr::Reg(reg) => {
                                    self.emit_func_data(format!("    mov rsi, [{}]", reg));
                                }
                            }

                            let field = self
                                .structs
                                .get(struct_name)
                                .unwrap()
                                .elements
                                .get(name)
                                .unwrap()
                                .clone();
                            self.emit_func_data(format!("    add rsi, {}", field.offset));
                            (Addr::Reg("rsi".to_string()), field.ty.clone())
                        }
                        _ => self::panic!("field access on non-struct pointer"),
                    },
                    Type::Struct(struct_name) => {
                        let layout = self
                            .structs
                            .get(&struct_name)
                            .expect("no struct with that name");

                        let field = layout.elements.get(name).expect("no such field in struct");
                        let field_type = field.ty.clone();

                        match addr {
                            Addr::Stack(pos) => {
                                // subtract offset for stack-down layout
                                (Addr::Stack(pos - field.offset as isize), field_type)
                            }

                            Addr::Reg(reg) => {
                                // subtract offset from register base address
                                self.emit_func_data(format!("    add {}, {}", reg, field.offset));
                                (Addr::Reg(reg), field_type)
                            }
                        }
                    }

                    _ => self::panic!("field access on non-struct"),
                }
            }

            LValue::Deref(inner) => {
                let (addr, ty) = self.calc_lvalue(inner);
                match ty {
                    Type::Pointer(inner_ty) => {
                        match &addr {
                            Addr::Stack(pos) => {
                                self.emit_func_data(format!("    mov rsi, [rbp - {}]", pos));
                            }
                            Addr::Reg(reg) => {
                                self.emit_func_data(format!("    mov rsi, [{}]", reg));
                            }
                        }
                        (Addr::Reg("rsi".to_string()), *inner_ty)
                    }
                    _ => self::panic!("deref of non-pointer"),
                }
            }
            LValue::Index { base, index } => {
                let (addr, ty) = self.calc_lvalue(base);
                let index_reg = self.eval_expr(index, &ty); // evaluate index
                match &ty {
                    Type::Array(ty, size) => {
                        self.emit_func_data(format!("    cmp {}, {}", index_reg, size));
                        self.emit_func_data(format!("    jge __bounds_fail__"));
                        self.emit_func_data(format!("    cmp {}, 0", index_reg));
                        self.emit_func_data(format!("    jl __bounds_fail__"));
                    }
                    _ => {}
                }
                // assume element size is 4 bytes (adjust if needed)
                let elem_size = match ty {
                    Type::Primitive(TokenType::IntType) => 4,
                    Type::Primitive(TokenType::ShortType) => 2,
                    Type::Primitive(TokenType::CharType) => 1,
                    _ => 8, // for pointers / structs
                };
                self.emit_func_data(format!(
                    "    lea rax, [{} + {} * {}]",
                    match addr {
                        Addr::Stack(pos) => format!("rbp - {}", pos),
                        Addr::Reg(reg) => reg.clone(),
                    },
                    index_reg,
                    elem_size
                ));
                (Addr::Reg("rax".to_string()), ty)
            }
        }
    }

    fn gen_assignment(&mut self, target: &LValue, value: &Expr) {
        let (addr, ty) = self.calc_lvalue(target);
        let val_reg = self.eval_expr(value, &ty);
        let sized_reg = reg_for_size("rax", &ty).unwrap();
        match addr {
            Addr::Stack(pos) => {
                let size_word = get_word(&ty);
                self.emit_func_data(format!(
                    "    mov {} [rbp - {}], {}",
                    size_word, pos, sized_reg
                ));
            }
            Addr::Reg(reg) => {
                let size_word = get_word(&ty);

                let sized_reg = reg_for_size(&val_reg, &ty).unwrap();
                self.emit_func_data(format!("    mov {} [{}], {}", size_word, reg, sized_reg));
            }
        }
    }

    pub fn gen_if(&mut self, data: (&Expr, &Box<Stmt>, &Option<Box<Stmt>>)) {
        let (condition, if_block, else_block) = data;

        self.eval_expr(condition, &Type::Primitive(TokenType::LongType));
        self.emit_func_data(format!("    cmp rax, 0"));

        let id = self.get_id();

        if let Some(else_stmt) = else_block {
            self.emit_func_data(format!("    je else_{}", id));
            self.emit_func_data(format!("if_{}:", id));
            self.gen_stmt(if_block);
            self.emit_func_data(format!("    jmp end_if_{}", id));
            self.emit_func_data(format!("else_{}:", id));
            self.gen_stmt(else_stmt);
        } else {
            self.emit_func_data(format!("    je end_if_{}", id));
            self.emit_func_data(format!("if_{}:", id));
            self.gen_stmt(if_block);
        }
        self.emit_func_data(format!("end_if_{}:", id));
    }

    pub fn gen_while(&mut self, data: (&Expr, &Box<Stmt>)) {
        let (condition, body) = data;
        let id = self.get_id();
        self.emit_func_data(format!("while_{}:", id));
        self.eval_expr(condition, &Type::Primitive(TokenType::LongType));
        self.emit_func_data(format!("    cmp rax, 0"));
        self.emit_func_data(format!("    je end_while_{}", id));
        self.gen_stmt(&*body);
        self.emit_func_data(format!("    jmp while_{}", id));
        self.emit_func_data(format!("end_while_{}:", id));
    }

    pub fn gen_for(
        &mut self,
        data: (
            &Option<Box<Stmt>>,
            &Option<Expr>,
            &Option<Box<Stmt>>,
            &Box<Stmt>,
        ),
    ) {
        let (init, condition, update, body) = data;

        let id = self.get_id();
        self.scopes.push(HashMap::new());
        if let Some(init_stmt) = init {
            self.gen_stmt(init_stmt);
        }
        self.emit_func_data(format!("for_start_{}:", id));

        if let Some(cond_expr) = condition {
            self.eval_expr(cond_expr, &Type::Primitive(TokenType::LongType));
            self.emit_func_data(format!("    cmp rax, 0"));
            self.emit_func_data(format!("    je for_end_{}", id));
        }

        self.gen_stmt(&body);
        if let Some(update_stmt) = update {
            self.gen_stmt(update_stmt);
        }
        self.scopes.pop();
        self.emit_func_data(format!("    jmp for_start_{}", id));

        self.emit_func_data(format!("for_end_{}:", id));
    }

    fn gen_ret(&mut self, expr: &Option<Expr>) {
        if let Some(ret_expr) = expr {
            let ret_type = self.current_return_type.clone();
            self.eval_expr(ret_expr, &ret_type); // result in rax/eax/ax/al
        }
        self.emit_func_data("    mov rsp, rbp".to_string());
        self.emit_func_data("    pop rbp".to_string());
        self.emit_func_data("    ret".to_string());
    }

    pub fn gen_inline_asm(&mut self, data: &Vec<String>) {
        for i in data.iter() {
            let mut var_buf = String::new();
            let mut buf = String::new();
            let mut iter = i.chars();

            while let Some(j) = iter.next() {
                if j != '(' {
                    buf.push(j);
                } else {
                    while let Some(next) = iter.next() {
                        if next == ')' {
                            break;
                        } else {
                            var_buf.push(next);
                        }
                    }
                    let var = self.lookup_var(&var_buf);
                    buf.push_str(&format!("[rbp - {}]", var.stack_pos));
                }
            }
            self.emit_func_data(format!("    {}", buf));
        }
    }

    pub fn compile_args(&mut self, args: &Vec<Declaration>) {
        let arg_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        for (i, decl) in args.iter().enumerate() {
            if i >= arg_regs.len() {
                self::panic!("too many args, stack args not supported yet");
            }
            let pos = self.alloc_type(&decl.ty);
            let reg = reg_for_size(arg_regs[i], &decl.ty).unwrap();
            self.emit_func_data(format!("    mov [rbp - {}], {}", pos, reg));
            let map = self.scopes.last_mut().unwrap();
            map.insert(
                decl.name.clone(),
                VarData {
                    global_flag: false,
                    stack_pos: pos,
                    var_type: decl.ty.clone(),
                },
            );
        }
    }

    pub fn member_addr(&mut self, base: &Expr, field_name: &str) -> Type {
        let base_type = base.get_type(self);
        self.eval_expr(base, &base_type); // rax = pointer to struct

        let struct_name = match &base_type {
            Type::Pointer(inner) => match inner.as_ref() {
                Type::Struct(name) => name.clone(),
                _ => self::panic!("pointer to non-struct"),
            },
            _ => self::panic!("-> on non-pointer, use . instead"),
        };

        let struct_data = self.structs.get(&struct_name).unwrap().clone();
        let field = struct_data.elements.get(field_name).unwrap();
        self.emit_func_data(format!("    add rax, {}", field.offset));
        field.ty.clone()
    }

    pub fn gen_func(
        &mut self,
        data: (&String, &Vec<Declaration>, &Type, &Box<Stmt>, &Vec<String>),
    ) {
        let saved_func_out = std::mem::take(&mut self.func_out);
        let saved_func_data = std::mem::take(&mut self.func_data);
        let saved_func_header = std::mem::take(&mut self.func_header);
        self.highest_stack_pos = 0;
        let (name, args, ret_type, body, generics) = data;
        self.current_return_type = ret_type.clone();
        // save outer scopes, start fresh with globals only
        let global_scope = self.scopes[0].clone();
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![global_scope]);
        let saved_stack = self.stack_pos;
        let overload_pos = self
            .functions
            .get(name)
            .unwrap()
            .iter()
            .position(|func| {
                func.args.len() == args.len()
                    && args
                        .iter()
                        .enumerate()
                        .all(|(i, decl)| func.args[i].ty == decl.ty)
            })
            .expect(&format!("no matching overload for '{}'", name));
        if self.functions.get(name).unwrap().len() > 1 {
            self.emit_func_header(format!("{}___{}:", name, overload_pos));
        } else if generics.len() > 0 {
            return;
        } else {
            self.emit_func_header(format!("{}:", name));
        }
        self.compile_args(args);
        self.gen_stmt(body);

        // restore outer scopes
        self.scopes = saved_scopes;
        self.stack_pos = saved_stack;
        match ret_type {
            Type::Primitive(ty) if *ty == TokenType::Void => {
                self.emit_func_data("    mov rsp, rbp".to_string());
                self.emit_func_data("    pop rbp".to_string());
                self.emit_func_data("    ret".to_string());
            }
            _ => {}
        }
        self.emit_func_header("    push rbp".to_string());
        self.emit_func_header("    mov rbp, rsp".to_string());
        self.emit_func_header(format!("    sub rsp, {}", align16(self.highest_stack_pos)));

        self.emit_func(self.func_header.clone());
        self.emit_func(self.func_data.clone());
        self.emit_main(self.func_out.clone());
        self.func_data = saved_func_data;
        self.func_header = saved_func_header;
        self.func_out = saved_func_out;
    }

    pub fn gen_init_struct(&mut self, data: &StructDef) {
        let mut elements = HashMap::new();
        for field in &data.fields {
            elements.insert(field.name.clone(), field.clone());
        }

        let struct_data = StructData {
            name: data.name.clone(),
            generic_type: data.generic_type.clone(),
            elements,
            byte_size: data.size,
        };

        self.structs.insert(data.name.clone(), struct_data);
    }

    pub fn type_size(&self, ty: &Type) -> usize {
        match ty {
            Type::Primitive(token) => match token {
                TokenType::CharType => 1,
                TokenType::ShortType => 2,
                TokenType::IntType => 4,
                TokenType::LongType => 8,
                _ => self::panic!("Unsupported primitive type: {:?}", token),
            },
            Type::Pointer(_) => 8,
            Type::Array(elem_type, count) => self.type_size(elem_type) * *count,
            Type::Struct(name) => {
                self.structs
                    .get(name)
                    .expect(&format!("Unknown struct: {}", name))
                    .byte_size
            }
            Type::Enum(name) => self.enum_get_size(name),
            Type::GenericType(name) => {
                let ty = self.generics.get(name).unwrap();
                self.type_size(ty)
            }
            Type::Unknown | Type::GenericInst(..) => {
                println!("{:?}", ty);
                self::panic!("unkown type")
            }
        }
    }

    fn type_to_data_directive(&self, ty: &Type) -> &str {
        match self.type_size(ty) {
            8 => "dq",
            4 => "dd",
            2 => "dw",
            1 => "db",
            _ => "dq", // default to 8 for unknown/structs/arrays
        }
    }

    fn gen_global(&mut self, global: Box<Stmt>) {
        match *global {
            Stmt::Declaration(decl_data) => {
                if let Some(_) = decl_data.initializer {
                    self::panic!("global cant have expr");
                }
                self.emit_data(format!(
                    "{} {} 0",
                    decl_data.name,
                    self.type_to_data_directive(&decl_data.ty)
                ));
                let global_var_data = VarData {
                    global_flag: true,
                    stack_pos: 0,
                    var_type: decl_data.ty.clone(),
                };

                self.global_vars
                    .insert(decl_data.name.clone(), global_var_data);
                if let Some(expr_data) = &decl_data.initializer {
                    self.eval_expr(expr_data, &decl_data.ty);
                    match decl_data.ty {
                        Type::Primitive(_) | Type::Pointer(_) => {
                            self.emit_func_data(format!("    mov [rel {}], rax", decl_data.name));
                        }
                        _ => {}
                    }
                }
            }
            _ => self::panic!("trying to make global of strange stmt"),
        }
    }

    fn gen_match_field_arg(&mut self, var_ty: &Type, field: &StructField, reg: &String, pos: &mut usize) {
        // the tag offset
        *pos += 8;
        match var_ty {
            Type::Primitive(_) => {
                match field.ty {
                    Type::Primitive(_) | Type::Array(..) => {
                        self.emit_func_data(format!("    mov {reg}, [rbp - {pos}]"));
                    }
                    Type::Unknown => {
                        self::panic!("some error");
                    }
                    _ => {
                        self.emit_func_data(format!("    lea {reg}, [rbp - {pos}]"));
                    }
                }
            }
            Type::Pointer(ty) => {
                self.emit_func_data(format!("    mov rax, [rbp - {}]",pos));
                self.emit_func_data(format!("    add rax, {}",field.offset));
                self.emit_func_data(format!("    mov rax, [rax]"));
                self.gen_match_field_arg(ty, field, reg, pos);
            }
            _ => {},
        }
    }

    fn gen_match_field(&mut self, variant: &MatchField, var_name: &String, expr_ty: &Type) {
        match &variant.left {
            MatchLeftValue::Enum { base, value, args } => {
                if base == "_" || args.len() < 1 {
                    self.gen_stmt(&variant.right);
                    return;
                }
                let new_base = {
                    match expr_ty {
                        Type::Enum(name) => name,
                        _ => base,
                    }
                };
                let var_data = self.lookup_var(var_name);
                let var_ty = &var_data.var_type.clone();
                let var_pos = var_data.stack_pos;
                let enum_data = self.enums.get(new_base).unwrap().clone();
                let field_data = enum_data.variants.get(value).unwrap();
                for (index, arg) in args.iter().enumerate() {
                    let field = &field_data.args[index];
                    let ty = {
                        match field.ty {
                            Type::Struct(_) | Type::Enum(_) => {
                                Type::Pointer(Box::new(field.ty.clone()))
                            }
                            _ => field.ty.clone(),
                        }
                    };
                    let decl = Declaration {
                        name: arg.clone(),
                        ty: ty,
                        initializer: None,
                    };
                    self.gen_declaration(&decl);
                    let new_var_pos = self.stack_pos;
                    // the tag size
                    let mut pos = var_pos - field.offset;
                    let reg = reg_for_size("rax", &field.ty).unwrap();
                    self.gen_match_field_arg(var_ty, &field, &reg, &mut pos);
                    self.emit_func_data(format!("    mov [rbp - {new_var_pos}], {reg}"));
                }
                self.gen_stmt(&variant.right);
            }
            MatchLeftValue::Expr { .. } => {
                self.gen_stmt(&variant.right);
            }
        }
    }

    fn gen_match_asm_checking(&mut self, var: &MatchField, id: usize, expr_ty: &Type) {
        match &var.left {
            MatchLeftValue::Expr { expr } => match expr {
                Expr::Number(num) => {
                    self.emit_func_data(format!("    cmp rax, {num}"));
                    self.emit_func_data(format!("    je match_{}_{}", id, num));
                }
                _ => self::panic!("match field left value not supported"),
            },
            MatchLeftValue::Enum { base, value, args } => {
                if base == "_" {
                    self.emit_func_data(format!("    jmp match_{id}_wildcard"));
                    return;
                }
                let new_base = {
                    match expr_ty {
                        Type::Enum(name) => name,
                        _ => base,
                    }
                };
                let enum_data = self.enums.get(new_base).unwrap();
                let field_data = enum_data.variants.get(value).unwrap();
                let tag = field_data.tag;
                self.emit_func_data(format!("    cmp rax, {}", tag));
                self.emit_func_data(format!("    je match_{}_{}", id, tag));
            }
        }
    }

    fn gen_match_asm_func(
        &mut self,
        variant: &MatchField,
        id: usize,
        expr_var_name: &String,
        expr_ty: &Type,
    ) {
        match &variant.left {
            MatchLeftValue::Expr { expr } => match expr {
                Expr::Number(num) => {
                    self.emit_func_data(format!("match_{}_{}:", id, num));
                }
                _ => self::panic!("not supported"),
            },
            MatchLeftValue::Enum { base, value, args } => {
                if base == "_" {
                    self.emit_func_data(format!("match_{id}_wildcard:"));
                } else {
                    let new_base = {
                        match expr_ty {
                            Type::Enum(name) => name,
                            _ => base,
                        }
                    };
                    let enum_data = self.enums.get(new_base).unwrap();
                    let field_data = enum_data.variants.get(value).unwrap();
                    let tag = field_data.tag;
                    self.emit_func_data(format!("match_{id}_{tag}:"));
                }
            }
        }
        self.scopes.push(HashMap::new());
        self.gen_match_field(&variant, expr_var_name, expr_ty);
        self.emit_func_data(format!("    jmp match_end_{id}"));
        self.scopes.pop();
    }

    fn resolve_match_expr(
        &mut self,
        expr: &Expr,
        variants: &Vec<MatchField>,
        id: usize,
        expr_ty: &Type,
    ) {
        match expr {
            Expr::Variable(var_name) => {
                for var in variants {
                    self.gen_match_asm_checking(var, id, &expr_ty);
                }
                self.emit_func_data(format!("    jmp match_end_{id}"));
                for var in variants {
                    self.gen_match_asm_func(var, id, var_name, &expr_ty);
                }
                self.emit_func_data(format!("match_end_{}:", id));
            }
            Expr::Deref(deref_expr) => {
                self.resolve_match_expr(deref_expr, variants, id, expr_ty);
            }
            _ => self::panic!("no supported match expr"),
        }
    }

    fn gen_match(&mut self, expr: &Expr, variants: &Vec<MatchField>) {
        let id = self.get_id();
        self.eval_expr(expr, &expr.get_type(self));
        let expr_ty = expr.get_type(self);
        self.resolve_match_expr(expr, variants, id, &expr_ty);
    }

    pub fn gen_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(v) => self.gen_block(v),
            Stmt::Declaration(v) => self.gen_declaration(v),
            Stmt::Assignment { target, value } => self.gen_assignment(target, value),
            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr, &Type::Primitive(TokenType::LongType));
            }
            Stmt::If {
                condition,
                if_block,
                else_block,
            } => {
                self.gen_if((condition, if_block, else_block));
            }
            Stmt::While { condition, body } => {
                self.gen_while((condition, body));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                self.gen_for((init, condition, update, body));
            }
            Stmt::Return(expr) => self.gen_ret(expr),
            Stmt::AsmCode(data) => self.gen_inline_asm(data),
            Stmt::InitFunc {
                name,
                args,
                ret_type,
                data,
                generic_types,
            } => self.gen_func((name, args, ret_type, data, generic_types)),
            Stmt::InitStruct(..) => {} // skiping because we already added it in first iteration,
            Stmt::GlobalDecl(global) => self.gen_global(global.clone()),
            Stmt::InitEnum { .. } => {}
            Stmt::Match { expr, variants } => self.gen_match(expr, variants),
        }
    }
}
