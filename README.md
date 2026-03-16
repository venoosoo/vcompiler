# vcompiler

A compiler for a custom C-like language, written from scratch in Rust. Compiles `.v` source files directly to x86-64 NASM assembly, which can be assembled and linked into a native Linux executable.

> **Status:** Work in progress. Core pipeline is functional — see [what's working](#whats-working) below.

---

## What it does

Takes `.v` source code like this:

```c
fn print_num(int n) -> int {
    if n < 0 {
        asm {
            "sub rsp, 1"
            "mov byte [rsp], '-'"
            "mov rax, 1"
            "mov rdi, 1"
            "mov rsi, rsp"
            "mov rdx, 1"
            "syscall"
            "add rsp, 1"
        }
        n = -n;
    }
    if n >= 10 {
        print_num(n / 10);
    }
    char c = (n % 10) + '0';
    syscall(1, 1, &c, 1, 0, 0, 0);
    return 0;
}
```

And produces real x86-64 NASM assembly:

```nasm
print_num:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov [rbp - 4], edi
    mov eax, DWORD [rbp - 4]
    mov ebx, 0
    cmp eax, ebx
    setl al
    movzx eax, al
    cmp eax, 0
    je end_if_1
if_1:
    sub rsp, 1
    mov byte [rsp], '-'
    ...
```

---

## Pipeline

```
.v source file
    │
    ▼
 Tokenizer        — lexes keywords, operators, literals, identifiers
    │
    ▼
  Parser          — recursive descent, builds typed AST
    │
    ▼
   IR / AST       — typed statement and expression tree
    │
    ▼
Semantic Analysis — type checking, variable resolution
    │
    ▼
 Code Generator   — walks AST, emits NASM x86-64 assembly
    │
    ▼
  main.asm        — assemble with nasm + ld
```

---

## What's working

- ✅ Tokenizer — full lexer for all language tokens
- ✅ Parser — recursive descent parser with Pratt-style expression parsing
- ✅ AST / IR — typed expression and statement nodes
- ✅ Semantic analysis — basic type checking and variable resolution
- ✅ Code generation — x86-64 NASM output, System V ABI compliant
- ✅ Primitive types: `int`, `char`, `short`, `long`
- ✅ Pointers and dereferencing
- ✅ Arrays with index access
- ✅ Structs with field access
- ✅ Functions with arguments and return values
- ✅ Control flow: `if`/`else`, `while`, `for`
- ✅ Inline `asm {}` blocks
- ✅ Arithmetic and comparison operators
- ✅ Recursion

## In progress / planned

- 🔧 String literals / string handling
- 🔧 Standard library (print, memory allocation)
- 🔧 Fully Working semantic analysis

---

## Language syntax

```c
// Functions
fn add(int a, int b) -> int {
    return a + b;
}

// Variables and types
int x = 42;
char c = 'A';
long big = 1000000;

// Pointers
int* ptr = &x;
int val = *ptr;

// Arrays
int arr[10];
arr[0] = 1;

// Structs
struct Point {
    int x;
    int y;
}
Point p = { x: 1, y: 2 };

// Control flow
if x > 10 {
    // ...
} else {
    // ...
}

while x > 0 {
    x = x - 1;
}

for (int i = 0; i < 10; i = i + 1) {
    // ...
}

// Inline assembly
asm {
    "mov rax, 60"
    "xor rdi, rdi"
    "syscall"
}
```

---

## Build & run

**Requirements:** Rust, NASM, ld (Linux only)

```bash
# Build the compiler
cargo build --release

# Compile a .v file
./target/release/vcompiler --file your_program.v

# Assemble and link the output
nasm -f elf64 main.asm -o main.o
ld main.o -o main

# Run
./main
```

---

## Project structure

```
src/
├── main.rs              — CLI entry point (clap)
├── Tokenizer/           — lexer
├── Parser/              — recursive descent parser
│   ├── expr.rs          — expression parsing (Pratt precedence climbing)
│   ├── stmt.rs          — statement parsing
│   └── function.rs      — function definition parsing
├── Ir/                  — AST/IR types and semantic analysis
│   ├── stmt.rs          — statement and type definitions
│   ├── expr.rs          — expression definitions
│   └── sem_analysis.rs  — type checker
└── Gen/                 — x86-64 code generator
    ├── gen_stmt.rs      — statement codegen
    └── gen_expr.rs      — expression codegen
```

---

## Why

Built to learn how compilers work end-to-end — from lexing raw source text to emitting assembly that runs on real hardware. Every stage was implemented manually without compiler frameworks or parser generators.
