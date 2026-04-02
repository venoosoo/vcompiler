use core::panic;
use std::collections::HashSet;
use std::path::PathBuf;
use std::{collections::HashMap, fmt::Write};

use crate::Ir::Stmt;
use crate::Ir::expr::Expr;
use crate::Ir::r#gen::*;
use crate::Ir::sem_analysis::Analyzer;
use crate::Ir::stmt::Type;
use crate::Ir::stmt::{EnumData, LValue};
use crate::Tokenizer::TokenType;

use crate::Ir::sem_analysis::SemanticError;

mod gen_expr;
mod gen_stmt;

fn align16(n: usize) -> usize {
    (n + 15) & !15
}

pub fn type_name(ty: &Type) -> String {
    match ty {
        Type::Primitive(token) => match token {
            TokenType::IntType => "int".to_string(),
            TokenType::LongType => "long".to_string(),
            TokenType::CharType => "char".to_string(),
            TokenType::ShortType => "short".to_string(),
            TokenType::Void => "void".to_string(),
            _ => format!("{:?}", token),
        },
        Type::Pointer(inner) => format!("{}*", type_name(inner)),
        Type::Array(inner, size) => format!("{}[{}]", type_name(inner), size),
        Type::Struct(name) => name.clone(),
        Type::Enum(name) => name.clone(),
        Type::GenericType(name) => name.clone(),
        Type::GenericInst(name, types) => {
            let type_args = types
                .iter()
                .map(|t| type_name(t))
                .collect::<Vec<_>>()
                .join("_");
            format!("{}__{}", name, type_args)
        }
        Type::Unknown => "unknown".to_string(),
    }
}

fn to_base_reg(reg: &str) -> &str {
    match reg {
        "eax" | "ax" | "al" => "rax",
        "ebx" | "bx" | "bl" => "rbx",
        "ecx" | "cx" | "cl" => "rcx",
        "edx" | "dx" | "dl" => "rdx",
        "esi" | "si" | "sil" => "rsi",
        "edi" | "di" | "dil" => "rdi",
        _ => reg, // already 64-bit or r8-r15
    }
}

pub fn reg_for_size(base: &str, ty: &Type) -> Option<String> {
    let base = to_base_reg(base);
    let size = match ty {
        Type::Primitive(token) => match token {
            TokenType::CharType => 1,
            TokenType::ShortType => 2,
            TokenType::IntType => 4,
            TokenType::LongType => 8,
            _ => return None,
        },

        Type::Unknown | Type::GenericType(_) | Type::GenericInst(..) => return None,
        Type::Pointer(_) | Type::Array(_, _) | Type::Struct(_) | Type::Enum(_) => 8,
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

pub fn arg_pos(pos: usize, ty: &Type) -> String {
    let size = match ty {
        Type::Primitive(token) => match token {
            TokenType::CharType => 1,
            TokenType::ShortType => 2,
            TokenType::IntType => 4,
            TokenType::LongType => 8,
            _ => panic!("unsupported primitive type in arg_pos: {:?}", token),
        },
        Type::Unknown | Type::GenericType(_) | Type::GenericInst(..) => panic!("unkown type: {:?}",ty),
        Type::Pointer(_) | Type::Array(_, _) | Type::Struct(_) | Type::Enum(_) => 8,
    };

    match (pos, size) {
        (0, 8) => "rdi",
        (0, 4) => "edi",
        (0, 2) => "di",
        (0, 1) => "dil",
        (1, 8) => "rsi",
        (1, 4) => "esi",
        (1, 2) => "si",
        (1, 1) => "sil",
        (2, 8) => "rdx",
        (2, 4) => "edx",
        (2, 2) => "dx",
        (2, 1) => "dl",
        (3, 8) => "rcx",
        (3, 4) => "ecx",
        (3, 2) => "cx",
        (3, 1) => "cl",
        (4, 8) => "r8",
        (4, 4) => "r8d",
        (4, 2) => "r8w",
        (4, 1) => "r8b",
        (5, 8) => "r9",
        (5, 4) => "r9d",
        (5, 2) => "r9w",
        (5, 1) => "r9b",
        (6, 8) => "r10",
        (6, 4) => "r10d",
        (6, 2) => "r10w",
        (6, 1) => "r10b",
        (7, 8) => "r11",
        (7, 4) => "r11d",
        (7, 2) => "r11w",
        (7, 1) => "r11b",
        _ => panic!("arg_pos: unsupported pos={} size={}", pos, size),
    }
    .to_string()
}

pub fn get_word(ty: &Type) -> String {
    match ty {
        Type::Primitive(token) => match token {
            TokenType::CharType => "BYTE".to_string(),
            TokenType::ShortType => "WORD".to_string(),
            TokenType::IntType => "DWORD".to_string(),
            TokenType::LongType => "QWORD".to_string(),
            _ => panic!("Unsupported primitive type: {:?}", token),
        },
        Type::Pointer(_) => "QWORD".to_string(), // 64-bit pointer
        Type::Array(_, _) => "QWORD".to_string(), // arrays decay to pointer for memory access
        Type::Struct(struct_name) => "QWORD".to_string(),
        Type::Enum(_) => "QWORD".to_string(),
        Type::Unknown | Type::GenericType(_) | Type::GenericInst(..) => panic!("unkown type"),
    }
}

pub fn lvalue_root(lvalue: &LValue) -> String {
    match lvalue {
        LValue::Variable(name) => name.clone(),
        LValue::Field { base, .. } => lvalue_root(base),
        LValue::Deref(inner) => lvalue_root(inner),
        LValue::Index { base, .. } => lvalue_root(base),
    }
}

impl Gen {
    pub fn new(stmts: Vec<Stmt>) -> Gen {
        Gen {
            stmts,
            current_return_type: Type::Primitive(TokenType::IntType),
            main_code: Vec::new(),
            data_code: Vec::new(),
            scopes: vec![HashMap::new()],
            stack_pos: 0,
            structs: HashMap::new(),
            functions: HashMap::new(),
            out: String::new(),
            generics: HashMap::new(),
            highest_stack_pos: 0,
            func_header: String::new(),
            func_out: String::new(),
            generic_func: HashMap::new(),
            func_data: String::new(),
            global_vars: HashMap::new(),
            enums: HashMap::new(),
            id: 0,
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
        self.emit("section .text".to_string());
        self.emit("global _start".to_string());
        self.emit("_start:".to_string());
        self.emit("    sub rsp, 8".to_string());
        self.emit("    call main".to_string());
        self.emit("    add rsp, 8".to_string());
        self.emit("    mov rax, 60".to_string());
        self.emit("    xor rdi, rdi".to_string());
        self.emit("    syscall".to_string());
        self.emit_all(self.main_code.clone());
        self.emit("__bounds_fail__:".to_string());
        self.emit("    mov rax, 60".to_string());
        self.emit("    mov rdi, 1".to_string());
        self.emit("    syscall".to_string());
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

    pub fn add_var(&mut self, var_data: VarData, name: String) {
        let last_scope = self.scopes.last_mut().unwrap();
        last_scope.insert(name, var_data);
    }

    pub fn reg_inits(&mut self, stmt: &Vec<Stmt>) {
        for i in stmt.iter() {
            match i {
                Stmt::InitFunc {
                    name,
                    args,
                    ret_type,
                    data,
                    generic_types,
                } => {
                    let func_data = FuncData {
                        args: args.clone(),
                        generic: generic_types.clone(),
                        return_type: ret_type.clone(),
                    };
                    if generic_types.len() > 0 {
                        let suka = Stmt::InitFunc {
                            name: name.clone(),
                            generic_types: generic_types.clone(),
                            args: args.clone(),
                            ret_type: ret_type.clone(),
                            data: data.clone(),
                        };
                        self.generic_func.insert(name.clone(), suka);
                    }
                    self.functions
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(func_data);
                }
                Stmt::InitStruct(data) => {
                    self.gen_init_struct(&data);
                }
                Stmt::InitEnum {
                    name,
                    variants,
                    generic_types,
                } => {
                    let enum_data = EnumData {
                        name: name.clone(),
                        generic_type: generic_types.clone(),
                        variants: variants.clone(),
                    };
                    self.enums.insert(name.clone(), enum_data);
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
