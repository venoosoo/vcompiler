use std::{collections::HashMap, dbg};

use crate::{
    Ir::stmt::{LValue, Type},
    tokenizer::TokenType,
};

pub fn substitute_type(ty: &Type, params: &Vec<String>, args: &Vec<Type>) -> Type {
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
            let new_args = inner_args
                .iter()
                .map(|a| substitute_type(a, params, args))
                .collect();
            Type::GenericInst(name.clone(), new_args)
        }
        _ => ty.clone(),
    }
}

pub fn align16(n: usize) -> usize {
    (n + 15) & !15
}

pub fn type_name(ty: &Type) -> String {
    match ty {
        Type::Primitive(token) => match token {
            TokenType::I32 => "i32".to_string(),
            TokenType::I64 => "i64".to_string(),
            TokenType::I8 => "i8".to_string(),
            TokenType::I16 => "i16".to_string(),
            TokenType::U32 => "u32".to_string(),
            TokenType::U64 => "u64".to_string(),
            TokenType::U8 => "u8".to_string(),
            TokenType::U16 => "u16".to_string(),
            TokenType::Void => "void".to_string(),
            _ => format!("{:?}", token),
        },
        Type::Pointer(inner) => format!("{}__ptr", type_name(inner)),
        Type::Array(inner, size) => format!("{}__arr__{}", type_name(inner), size),
        Type::Struct(name) => name.clone(),
        Type::Enum(name, _) => name.clone(),
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

pub fn to_base_reg(reg: &str) -> &str {
    match reg {
        "eax" | "ax" | "al" => "rax",
        "ebx" | "bx" | "bl" => "rbx",
        "ecx" | "cx" | "cl" => "rcx",
        "edx" | "dx" | "dl" => "rdx",
        "esi" | "si" | "sil" => "rsi",
        "edi" | "di" | "dil" => "rdi",
        _ => reg,
    }
}

pub fn arg_pos(pos: usize, ty: &Type) -> String {
    let size = match ty {
        Type::Primitive(token) => match token {
            TokenType::I8 => 1,
            TokenType::I16 => 2,
            TokenType::I32 => 4,
            TokenType::I64 => 8,
            _ => panic!("unsupported primitive type in arg_pos: {:?}", token),
        },
        Type::Unknown | Type::GenericType(_) | Type::GenericInst(..) => {
            panic!("unkown type: {:?}", ty)
        }
        Type::Pointer(_) | Type::Array(_, _) | Type::Struct(_) | Type::Enum(..) => 8,
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

pub fn lvalue_root(lvalue: &LValue) -> String {
    match lvalue {
        LValue::Variable(name) => name.clone(),
        LValue::Field { base, .. } => lvalue_root(base),
        LValue::Deref(inner) => lvalue_root(inner),
        LValue::Index { base, .. } => lvalue_root(base),
    }
}

pub fn is_numeric(ty: &Type) -> bool {
    numeric_rank(ty).is_some()
}

pub fn is_number(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(TokenType::I8)
            | Type::Primitive(TokenType::I16)
            | Type::Primitive(TokenType::I32)
            | Type::Primitive(TokenType::I64)
            | Type::Primitive(TokenType::U8)
            | Type::Primitive(TokenType::U16)
            | Type::Primitive(TokenType::U32)
            | Type::Primitive(TokenType::U64)
    )
}

pub fn coerce_numeric(a: &Type, b: &Type) -> Type {
    let a_info = numeric_rank(a).unwrap_or((0, true));
    let b_info = numeric_rank(b).unwrap_or((0, true));

    let (a_rank, a_is_signed) = a_info;
    let (b_rank, b_is_signed) = b_info;

    if a_rank > b_rank {
        a.clone()
    } else if b_rank > a_rank {
        b.clone()
    } else {
        if !a_is_signed {
            a.clone()
        } else if !b_is_signed {
            b.clone()
        } else {
            a.clone()
        }
    }
}

pub fn numeric_rank(ty: &Type) -> Option<(u8, bool)> {
    // (rank, is_signed)
    match ty {
        Type::Primitive(TokenType::I8) => Some((1, true)),
        Type::Primitive(TokenType::I16) => Some((2, true)),
        Type::Primitive(TokenType::I32) => Some((3, true)),
        Type::Primitive(TokenType::I64) => Some((4, true)),
        Type::Primitive(TokenType::U8) => Some((1, false)),
        Type::Primitive(TokenType::U16) => Some((2, false)),
        Type::Primitive(TokenType::U32) => Some((3, false)),
        Type::Primitive(TokenType::U64) => Some((4, false)),
        _ => None,
    }
}

pub fn check_types(left: &Type, right: &Type) -> bool {
    // 1. Strict equality (This handles i32 == i32 automatically)
    if left == right {
        return true;
    }

    // 2. Generics bypass
    if matches!(left, Type::GenericType(_)) || matches!(right, Type::GenericType(_)) {
        return true;
    }

    // 3. char array compatible with char* (u8[] <-> u8*)
    match (left, right) {
        (Type::Pointer(ptr_elem), Type::Array(arr_elem, _))
        | (Type::Array(arr_elem, _), Type::Pointer(ptr_elem)) => {
            if ptr_elem == arr_elem {
                return true;
            }
        }
        _ => {}
    }


    // 4. Allow variants without data to cast to other data types
    // Example Option::None can cast to any Option__i64, Option__i32, etc...
    if let (Type::Enum(l_name, _), Type::Enum(r_name, _)) = (left, right) {
        let right_is_base = !r_name.contains("__");
        let left_matches_base = l_name.starts_with(&format!("{}__", r_name));

        if right_is_base && left_matches_base {
            return true;
        }

        let left_is_base = !l_name.contains("__");
        let right_matches_base = r_name.starts_with(&format!("{}__", l_name));

        if left_is_base && right_matches_base {
            return true;
        }
    }

    // 5. void* compatible with any pointer
    let is_void_ptr = |t: &Type| *t == Type::Pointer(Box::new(Type::Primitive(TokenType::Void)));
    if is_void_ptr(left) && matches!(right, Type::Pointer(_)) {
        return true;
    }
    if is_void_ptr(right) && matches!(left, Type::Pointer(_)) {
        return true;
    }

    // 6. The variant should be deleted i think
    if let (Type::Enum(l_name, _l_variant), Type::Enum(r_name, _r_variant)) = (left, right) {
        if l_name == r_name {
            return true;
        }

        let right_is_base = !r_name.contains("__");
        let left_matches_base = l_name.starts_with(&format!("{}__", r_name));

        if right_is_base && left_matches_base {
            return true;
        }

        let left_is_base = !l_name.contains("__");
        let right_matches_base = r_name.starts_with(&format!("{}__", l_name));

        if left_is_base && right_matches_base {
            return true;
        }
    }

    false
}

pub fn aligned_size(total_size: usize, largest_align: usize) -> usize {
    (total_size + largest_align - 1) & !(largest_align - 1)
}

pub fn is_unsigned(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(TokenType::U8)
            | Type::Primitive(TokenType::U16)
            | Type::Primitive(TokenType::U32)
            | Type::Primitive(TokenType::U64)
    )
}

pub fn same_signedness(l: &Type, r: &Type) -> bool {
    match (numeric_rank(l), numeric_rank(r)) {
        (Some((_, l_signed)), Some((_, r_signed))) => l_signed == r_signed,
        _ => true,
    }
}

pub fn build_generic_map(
    generic_names: Vec<String>,
    concrete_types: Vec<Type>,
) -> HashMap<String, Type> {
    if generic_names.len() != concrete_types.len() {
        panic!(
            "Generic argument mismatch: expected {} arguments, found {}",
            generic_names.len(),
            concrete_types.len()
        );
    }

    generic_names
        .into_iter()
        .zip(concrete_types.into_iter())
        .collect()
}
