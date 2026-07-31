use core::panic;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::{collections::HashMap, fmt::Write};
use std::{dbg, matches};

use crate::Ir::Stmt;
use crate::Ir::expr::{Expr, ExprType, Lookup};
use crate::Ir::r#gen::*;
use crate::Ir::sem_analysis::Analyzer;
use crate::Ir::shared::TypeContext;
use crate::Ir::stmt::{EnumData, LValue, StmtType};
use crate::Ir::stmt::{EnumVariant, StructField, Type};
use crate::shared::{
    aligned_size, build_generic_map, check_types, is_number, substitute_type, to_base_reg,
    transform_generic_name, type_name,
};
use crate::tokenizer::TokenType;

mod gen_expr;
mod gen_stmt;

const TAG_SIZE: usize = 8;

impl TypeContext for Gen {
    fn resolve_call(
        &self,
        name: &String,
        args: &Vec<Expr>,
        generics: &Vec<Type>,
    ) -> Option<(FuncData, usize)> {
        let vec_func_data = self.functions.get(name).unwrap().clone();

        if generics.len() > 0 {
            let (overload_pos, func_data) = self
                .find_overload(&vec_func_data, args, generics)
                .expect(&format!("no matching overload for function '{}'", name));
            return Some((func_data, overload_pos));
        }

        let (overload_pos, func_data) = self
            .find_overload(&vec_func_data, args, &Vec::new())
            .expect(&format!("no matching overload for function '{}'", name));
        Some((func_data, overload_pos))
    }

    fn field_alignment(&self, ty: &Type) -> usize {
        match ty {
            Type::Struct(name) => {
                let s = self.structs.borrow().get(name).cloned().unwrap();
                s.elements
                    .values()
                    .map(|f| self.field_alignment(&f.ty))
                    .max()
                    .unwrap_or(1)
            }
            Type::Enum(name, _) => {
                let e = self.enums.borrow().get(name).cloned().unwrap();
                let variant_align = e
                    .variants
                    .values()
                    .flat_map(|v| v.args.iter())
                    .map(|f| self.field_alignment(&f.ty))
                    .max()
                    .unwrap_or(1);
                8usize.max(variant_align)
            }
            _ => self.type_size(ty), // primitives: size == alignment
        }
    }

    fn monomorphize_struct(&self, def: &StructData, type_args: &Vec<Type>) -> Type {
        let mangled = format!(
            "{}__{}",
            def.name,
            type_args
                .iter()
                .map(|t| type_name(t))
                .collect::<Vec<_>>()
                .join("_")
        );
        if self.structs.borrow().contains_key(&mangled) {
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
        let size = self.compute_struct_size(&fields);
        self.structs.borrow_mut().insert(
            mangled.clone(),
            StructData {
                generic_type: Vec::new(),
                name: mangled.clone(),
                elements: fields.iter().map(|f| (f.name.clone(), f.clone())).collect(),
                size,
            },
        );
        return Type::Struct(mangled);
    }

    fn monomorphize_enum(&self, def: &EnumData, type_args: &Vec<Type>) -> Type {
        let mangled = transform_generic_name(&def.name, type_args, -1);

        if self.enums.borrow().contains_key(&mangled) {
            return Type::Enum(mangled.clone(), None);
        }

        let mut new_variants = HashMap::new();
        let mut max_size = TAG_SIZE;

        for (var_name, variant) in def.variants.iter() {
            let mut current_offset = TAG_SIZE;
            let mut new_args = Vec::new();

            for arg in variant.args.iter() {
                let actual_ty = substitute_type(&arg.ty, &def.generic_type, type_args);

                new_args.push(StructField {
                    name: arg.name.clone(),
                    ty: actual_ty.clone(),
                    offset: current_offset,
                });

                current_offset += self.type_size(&actual_ty);
            }

            let variant_size = current_offset;

            new_variants.insert(
                var_name.clone(),
                EnumVariant {
                    name: variant.name.clone(),
                    tag: variant.tag,
                    args: new_args,
                    size: variant_size,
                },
            );

            if variant_size > max_size {
                max_size = variant_size;
            }
        }

        self.enums.borrow_mut().insert(
            mangled.clone(),
            EnumData {
                name: mangled.clone(),
                generic_type: Vec::new(),
                variants: new_variants,
                size: max_size,
            },
        );

        return Type::Enum(mangled, None);
    }

    fn ensure_monomorphized(&self, ty: &Type) -> Type {
        match ty {
            Type::GenericInst(name, type_args) => {
                let mangled = type_name(ty);
                if self.structs.borrow().contains_key(&mangled) {
                    return Type::Struct(mangled.clone());
                }
                if self.enums.borrow().contains_key(&mangled) {
                    return Type::Enum(mangled.clone(), None);
                }
                // find the generic definition and monomorphize
                let struct_def = self.structs.borrow().get(name).cloned();
                let enum_def = self.enums.borrow().get(name).cloned();
                if let Some(struct_def) = struct_def {
                    return self.monomorphize_struct(&struct_def, type_args);
                } else if let Some(enum_def) = enum_def {
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
}

impl Expr {
    /// Returns the Type of this expression
    pub fn get_type(&self, helper: &impl Lookup) -> Type {
        match &self.ty {
            ExprType::Number(_) => Type::Primitive(TokenType::I64),
            ExprType::Float(_) => todo!(),
            ExprType::Variable(var_name) => helper
                .look_var(var_name)
                .unwrap_or(Type::Primitive(TokenType::I64)),
            ExprType::Binary { op, left, right } => helper.look_binary(op, left, right),
            ExprType::Unary { op, expr } => helper.look_unary(op, expr),
            ExprType::Call {
                name,
                args,
                generics,
            } => helper.look_call(name, args, generics),
            ExprType::StructInit {
                struct_name_ty,
                fields,
            } => helper.look_struct_init(struct_name_ty),
            ExprType::StructMember { base, name } => helper.look_struct_member(base, name),
            ExprType::Deref(expr) => helper.look_deref(expr),
            ExprType::Index { base, index } => helper.look_index(base, index),
            ExprType::ArrayInit { elements } => helper.look_array_init(elements),
            ExprType::SizeOf { ty } => Type::Primitive(TokenType::I64),
            ExprType::String { str } => {
                return Type::Array(Box::new(Type::Primitive(TokenType::U8)), str.len() + 1);
            }
            ExprType::GetEnum {
                base,
                variant,
                value,
            } => helper.look_get_enum(base, variant),
            ExprType::Cast { expr, ty } => ty.clone(),
        }
    }
}

impl Gen {
    pub fn new(stmts: Vec<Stmt>) -> Gen {
        Gen {
            stmts,
            current_return_type: Type::Primitive(TokenType::Void),
            main_code: Vec::new(),
            data_code: Vec::new(),
            scopes: vec![HashMap::new()],
            stack_pos: 0,
            contniue_stack: Vec::new(),
            break_stack: Vec::new(),
            structs: RefCell::new(HashMap::new()),
            functions: HashMap::new(),
            out: String::new(),
            generics: RefCell::new(HashMap::new()),
            highest_stack_pos: 0,
            bss_code: Vec::new(),
            func_header: String::new(),
            func_out: String::new(),
            generic_func: HashMap::new(),
            func_data: String::new(),
            global_vars: HashMap::new(),
            enums: RefCell::new(HashMap::new()),
            id: 0,
        }
    }

    pub fn reg_for_size(&self, base: &str, ty: &Type) -> Option<String> {
        let base = to_base_reg(base);
        let size = match ty {
            Type::Primitive(token) => match token {
                TokenType::I8 | TokenType::U8 => 1,
                TokenType::I16 | TokenType::U16 => 2,
                TokenType::I32 | TokenType::U32 => 4,
                TokenType::I64 | TokenType::U64 => 8,
                _ => return None,
            },
            Type::GenericType(name) => {
                let res = {
                    let map = self.generics.borrow();

                    map.get(name).cloned().unwrap()
                };
                return self.reg_for_size(base, &res);
            }

            Type::Unknown | Type::GenericInst(..) => return None,
            Type::Pointer(_) | Type::Array(_, _) | Type::Struct(_) | Type::Enum(..) => 8,
        };

        match (base, size) {
            ("rax", 8) => Some("rax".into()),
            ("rax", 4) => Some("eax".into()),
            ("rax", 2) => Some("ax".into()),
            ("rax", 1) => Some("al".into()),
            ("rbx", 8) => Some("rbx".into()),
            ("rbx", 4) => Some("ebx".into()),
            ("rbx", 2) => Some("bx".into()),
            ("rbx", 1) => Some("bl".into()),
            ("rcx", 8) => Some("rcx".into()),
            ("rcx", 4) => Some("ecx".into()),
            ("rcx", 2) => Some("cx".into()),
            ("rcx", 1) => Some("cl".into()),
            ("rdx", 8) => Some("rdx".into()),
            ("rdx", 4) => Some("edx".into()),
            ("rdx", 2) => Some("dx".into()),
            ("rdx", 1) => Some("dl".into()),
            ("rsi", 8) => Some("rsi".into()),
            ("rsi", 4) => Some("esi".into()),
            ("rsi", 2) => Some("si".into()),
            ("rsi", 1) => Some("sil".into()),
            ("rdi", 8) => Some("rdi".into()),
            ("rdi", 4) => Some("edi".into()),
            ("rdi", 2) => Some("di".into()),
            ("rdi", 1) => Some("dil".into()),
            (reg, 8) => Some(reg.to_string()),
            (reg, 4) if reg.starts_with('r') => Some(format!("{}d", reg)),
            (reg, 2) if reg.starts_with('r') => Some(format!("{}w", reg)),
            (reg, 1) if reg.starts_with('r') => Some(format!("{}b", reg)),
            _ => None,
        }
    }

    pub fn find_overload(
        &self,
        vec_func_data: &Vec<FuncData>,
        args: &Vec<Expr>,
        generics: &Vec<Type>,
    ) -> Option<(usize, FuncData)> {
        vec_func_data
            .iter()
            .enumerate()
            .find(|(_, func)| {
                if func.args.len() != args.len() {
                    return false;
                }
                args.iter().enumerate().all(|(i, expr)| {
                    let expr_ty = expr.get_type(self);
                    let param_ty = &func.args[i].ty.clone();
                    let expr_ty = self.ensure_monomorphized(&expr_ty);
                    let (expr_ty, param_ty) = {
                        let map = build_generic_map(&func.generic, generics);
                        let expr_ty: Type = self.generic_to_ty(&expr_ty, &map);
                        let param_ty = self.generic_to_ty(param_ty, &map);
                        (expr_ty, param_ty)
                    };
                    let param_ty = self.ensure_monomorphized(&param_ty);
                    let arg_matches = match &expr.ty {
                        ExprType::Number(_) => {
                            matches!(param_ty, Type::GenericType(_)) || is_number(&param_ty)
                        }
                        _ => check_types(&expr_ty, &param_ty),
                    };

                    arg_matches
                })
            })
            .map(|(pos, func)| (pos, func.clone()))
    }

    pub fn get_word(&self, ty: &Type) -> String {
        match ty {
            Type::Primitive(token) => match token {
                TokenType::I8 | TokenType::U8 => "BYTE".to_string(),
                TokenType::I16 | TokenType::U16 => "WORD".to_string(),
                TokenType::I32 | TokenType::U32 => "DWORD".to_string(),
                TokenType::I64 | TokenType::U64 => "QWORD".to_string(),
                _ => panic!("Unsupported primitive type: {:?}", token),
            },
            Type::Pointer(_) => "QWORD".to_string(), // 64-bit pointer
            Type::Array(_, _) => "QWORD".to_string(), // arrays decay to pointer for memory access
            Type::Struct(struct_name) => "QWORD".to_string(),
            Type::Enum(..) => "QWORD".to_string(),
            Type::GenericType(name) => {
                let res = {
                    let map = self.generics.borrow();
                    map.get(name).cloned().unwrap()
                };
                return self.get_word(&res);
            }
            Type::Unknown | Type::GenericInst(..) => panic!("unkown type"),
        }
    }

    fn emit(&mut self, s: String) {
        let _ = writeln!(self.out, "{}", s);
    }

    fn emit_func_header(&mut self, s: String) {
        let _ = writeln!(self.func_header, "{}", s);
    }

    fn emit_func_data(&mut self, s: String) {
        let _ = writeln!(self.func_data, "{}", s);
    }

    fn emit_bss(&mut self, s: String) {
        self.bss_code.push(s);
    }

    fn emit_func(&mut self, s: String) {
        let _ = writeln!(self.func_out, "{}", s);
    }

    fn emit_main(&mut self, s: String) {
        self.main_code.push(s);
    }

    fn emit_data(&mut self, s: String) {
        self.data_code.push(s);
    }

    fn emit_all(&mut self, s: Vec<String>) {
        for line in s {
            let _ = writeln!(self.out, "{}", line);
        }
    }

    fn get_id(&mut self) -> usize {
        self.id += 1;
        self.id
    }

    fn alloc_type(&mut self, ty: &Type) -> usize {
        let size: usize = self.type_size(ty);
        self.stack_pos += size;
        if self.highest_stack_pos < self.stack_pos {
            self.highest_stack_pos = self.stack_pos
        }
        self.stack_pos
    }

    fn alloc(&mut self, size: usize) -> usize {
        self.stack_pos += size;
        if self.highest_stack_pos < self.stack_pos {
            self.highest_stack_pos = self.stack_pos
        }
        self.stack_pos
    }

    pub fn gen_asm(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        self.gen_stmts();
        self.emit("section .data".to_string());
        self.emit_all(self.data_code.clone());
        self.emit("section .bss".to_string());
        self.emit_all(self.bss_code.clone());
        self.emit("section .text".to_string());
        self.emit("global _start".to_string());
        self.emit("_start:".to_string());
        self.emit("    mov rdi, [rsp]".to_string());
        self.emit("    lea rsi, [rsp+8]".to_string());
        self.emit("    call main".to_string());
        if self.current_return_type == Type::Primitive(TokenType::Void) {
            self.emit(format!("    mov rax, 0"));
        }
        self.emit("    mov rdi, rax".to_string());
        self.emit("    mov rax, 60".to_string());
        self.emit("    syscall".to_string());
        self.emit_all(self.main_code.clone());
        Ok(self.out.clone())
    }

    pub fn lookup_var(&self, name: &str) -> &VarData {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return ty;
            }
        }
        if let Some(global_var) = self.global_vars.get(name) {
            return global_var;
        }
        println!("scopes: {:?}", self.scopes);
        self::panic!("couldnt find the var with name: {}", name);
    }

    pub fn reg_inits(&mut self, stmt: &Vec<Stmt>) {
        for i in stmt.iter() {
            match &i.ty {
                StmtType::InitFunc {
                    name,
                    args,
                    ret_type,
                    data,
                    generic_types,
                } => {
                    let func_data = FuncData {
                        args: args.clone(),
                        generic: Vec::new(),
                        return_type: ret_type.clone(),
                    };
                    self.functions
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(func_data);
                }
                StmtType::GenericInitFunc {
                    name,
                    generic_types,
                    args,
                    ret_type,
                    data,
                } => {
                    let func_data = FuncData {
                        args: args.clone(),
                        generic: generic_types.clone(),
                        return_type: ret_type.clone(),
                    };
                    self.functions
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(func_data);
                    self.generic_func
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(i.clone());
                }
                StmtType::InitStruct(data) => {
                    self.gen_init_struct(&data);
                }
                StmtType::InitEnum {
                    name,
                    variants,
                    generic_types,
                } => {
                    let mut max_size = 0;
                    for (_, data) in variants.iter() {
                        if max_size < data.size {
                            max_size = data.size
                        }
                    }

                    let enum_data = EnumData {
                        name: name.clone(),
                        generic_type: generic_types.clone(),
                        variants: variants.clone(),
                        size: max_size + 8,
                    };
                    self.enums.borrow_mut().insert(name.clone(), enum_data);
                }
                _ => {}
            }
        }
    }

    fn gen_stmts(&mut self) {
        let stmt = std::mem::take(&mut self.stmts);

        self.reg_inits(&stmt);

        for i in stmt.iter() {
            self.gen_stmt(i);
        }
    }
}
