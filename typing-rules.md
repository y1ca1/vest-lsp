# Vest DSL Typing Rules

This document gives an informal but consolidated presentation of the typing
rules for the Vest DSL.

The high-level typing problem in Vest is:

```text
format specification  ─────►  host data type
       fmt                     type
```

A typing judgment assigns each Vest format expression a host-side Rust type
shape. Primitive integer formats map to Rust integer types, byte-oriented
formats map to byte slices or fixed arrays, sequence formats map to arrays or
`Vec<T>`, optional formats map to `Option<T>`, and structured or branching
formats map to Rust `struct` or `enum` shapes.

The core of the theory is still syntax-directed typing. In addition, this phase
can perform several local validation checks that are straightforward and do not
require deep semantic analysis, for example:

- integer literals fitting a concrete representation
- enum representation inference
- constant byte arrays matching their declared length and staying within `u8`
- basic refinement validation such as range ordering and literal-fit checks
- array-pattern length consistency
- exact static-size equality across alternative branches
- simple tag exhaustiveness checks
- distinct struct field identifiers

What remains out of scope here are deeper analyses such as branch
disjointness/unambiguity, cycles, recursion depth, non-malleability, and other
whole-program semantic properties.

## 1. Core Syntax

### 1.1 Primitive Carriers

```text
κ ::= u8 | u16 | u24 | u32 | u64
    | i8 | i16 | i24 | i32 | i64
    | btc_varint | uleb128
```

### 1.2 Host Types

```text
τ ::= prim(κ)
    | enum(F)
    | bytes
    | array(τ, n)
    | vec(τ)
    | option(τ)
    | struct(ρ̄)
    | choice(δ, β̄)

ρ ::= x : τ
    | const x : τc

β ::= π ↦ τ

τc ::= prim(κ)
     | enum(F)
     | array(prim(u8), n)

δ ::= •
    | prim(κ)
    | enum(F)
    | bytes
    | array(prim(u8), n)
```

`τ` is a host-side Rust type shape, not a source-level format.

- `prim(κ)` means the Rust integer type induced by `κ`.
- `enum(F)` means the nominal Rust enum introduced by a source definition
  `F = enum { ... }`.
- `bytes` means a dynamically sized byte sequence, corresponding to Rust
  `&[u8]`.
- `array(τ, n)` means a fixed Rust array `[T; n]`.
  `array(prim(u8), n)` is therefore the fixed-byte-array case.
- `vec(τ)` means `Vec<T>`.
- `struct(ρ̄)` and `choice(δ, β̄)` are structural precursors of generated Rust
  `struct` and `enum` items.

Every `τc` is also a host type shape, just restricted to the constant-carrying
fragment used by const fields, const definitions, and wrap arguments.

### 1.3 Paths and Length Expressions

```text
p ::= @x
    | p.f

e ::= n
    | p
    | |F|
    | e1 ⊕ e2        where ⊕ ∈ {+, -, *, /}
```

### 1.4 Refinements and Branch Patterns

```text
χ ::= v
    | v1..v2
    | ..v
    | v..
    | {χ1, ..., χn}
    | !χ

η ::= V
    | {V1, ..., Vn}
    | !η

π ::= _
    | χ
    | η
    | [v1, ..., vn]
    | L
```

### 1.5 Formats, Parameters, and Definitions

```text
f ::= κ
    | F
    | F(@x1, ..., @xn)
    | f | ψ
    | [f; e]
    | Vec<f>
    | Option<f>
    | { b1, ..., bn }
    | choose(s) { π1 => f1, ..., πn => fn }
    | wrap(k̄pre, f, k̄post)
    | tail
    | f1 >>= f2

ψ ::= χ | η

s ::= • | @x

b ::= @x : f
    | x : f
    | const x : τc = v

λ ::= ·
    | @x1 : f1, ..., @xn : fn

d ::= F = f
    | F(λ) = f
    | const F : τc = v
    | F = enum[openness] { V1 = n1, ..., Vm = nm }
```

`k̄pre` and `k̄post` range over constant wrap arguments, that is, wrap arguments
whose host shape is described by some constant carrier `τc`.

## 2. Contexts and Judgments

### 2.1 Global Signature

```text
Σ ::= ∅
    | Σ, F : fmt(Π, τ)
    | Σ, F : enum_sig(κ, openness, V̄)
    | Σ, F : const(τc)

Π ::= ·
    | @x1 : τ1, ..., @xn : τn
```

We write `ΔΠ` for the local context induced by `Π`.

For nullary top-level names, define the induced host type:

```text
host_tyΣ(F) = τ        if Σ(F) = fmt(·, τ)
host_tyΣ(F) = enum(F)  if Σ(F) = enum_sig(κ, openness, V̄)
```

### 2.2 Local Context

```text
Δ ::= ∅
    | Δ, @x : τ
```

Only dependent binders extend the local context.

### 2.3 Judgment Forms

| Judgment | Meaning |
| --- | --- |
| `Σ; Δ ⊢ f : τ` | format `f` maps to host type `τ` |
| `Σ ⊢ λ ⇒ Π` | parameter formats elaborate to parameter host types |
| `Σ; Δ ⊢ b ⇒ Δ' ; ρ` | binder `b` contributes record entry `ρ` and updated context `Δ'` |
| `Σ; Δ ⊢ p : τ` | path `p` resolves to host type `τ` |
| `Σ; Δ ⊢ e : nat` | length expression `e` is well-formed |
| `Σ; Δ ⊢ e ⇓ n` | length expression `e` has exact static value `n` |
| `Σ ⊢ v fits κ` | integer literal `v` fits representation `κ` |
| `Σ; Δ ⊢ ψ ok for τ` | refinement syntax `ψ` is valid for carrier `τ` |
| `Σ; Δ ⊢ π ok for δ` | branch pattern `π` is valid for discriminant class `δ` |
| `Σ; Δ ⊢ β̄ exhaustive for δ` | the choice arms satisfy the local exhaustiveness check for `δ` |
| `Σ ⊢ β̄ same_size` | all statically-sized alternative branches have the same exact size |
| `Σ ⊢ v : τc` | constant `v` has constant carrier `τc` |
| `Σ ⊢ τ ⇓size n` | host type `τ` has exact static byte size `n` |
| `Σ ⊢ n̄ ⇒repr κ` | enum literal set `n̄` induces representation `κ` |
| `Σ ⊢ d ok ⇒ Σ'` | definition `d` is well-typed and extends the signature to `Σ'` |
| `⊢ d̄ ok` | the whole program is well-typed |

## 3. Core Typing and Local Validation Rules

### 3.1 Parameter Lists, Paths, and Lengths

Parameter annotations are source formats, not host types.

```text
            ─────────────────────────────── [Params-Empty]
            Σ ⊢ · ⇒ ·


            ∀i. Σ; ∅ ⊢ fi : τi
            distinct(@x1, ..., @xn)
            ─────────────────────────────── [Params]
            Σ ⊢ (@x1 : f1, ..., @xn : fn)
                  ⇒ (@x1 : τ1, ..., @xn : τn)
```

```text
            @x : τ ∈ Δ
            ─────────────────────────────── [P-Var]
            Σ; Δ ⊢ @x : τ


            Σ; Δ ⊢ p : struct(..., x : τ, ...)
            ─────────────────────────────── [P-Proj]
            Σ; Δ ⊢ p.x : τ
```

```text
            n ∈ ℕ
            ─────────────────────────────── [E-Lit]
            Σ; Δ ⊢ n : nat


            Σ; Δ ⊢ p : prim(κ)
            ─────────────────────────────── [E-Path]
            Σ; Δ ⊢ p : nat


            host_tyΣ(F) = τ
            Σ ⊢ τ ⇓size n
            ─────────────────────────────── [E-SizeOf]
            Σ; Δ ⊢ |F| : nat


            Σ; Δ ⊢ e1 : nat
            Σ; Δ ⊢ e2 : nat
            ─────────────────────────────── [E-Arith]
            Σ; Δ ⊢ e1 ⊕ e2 : nat
```

Exact evaluation of statically-known length expressions:

```text
            n ∈ ℕ
            ─────────────────────────────── [Eval-Lit]
            Σ; Δ ⊢ n ⇓ n


            host_tyΣ(F) = τ
            Σ ⊢ τ ⇓size n
            ─────────────────────────────── [Eval-SizeOf]
            Σ; Δ ⊢ |F| ⇓ n


            Σ; Δ ⊢ e1 ⇓ n1
            Σ; Δ ⊢ e2 ⇓ n2
            n = n1 + n2
            ─────────────────────────────── [Eval-Add]
            Σ; Δ ⊢ e1 + e2 ⇓ n


            Σ; Δ ⊢ e1 ⇓ n1
            Σ; Δ ⊢ e2 ⇓ n2
            n1 ≥ n2
            n = n1 - n2
            ─────────────────────────────── [Eval-Sub]
            Σ; Δ ⊢ e1 - e2 ⇓ n


            Σ; Δ ⊢ e1 ⇓ n1
            Σ; Δ ⊢ e2 ⇓ n2
            n = n1 * n2
            ─────────────────────────────── [Eval-Mul]
            Σ; Δ ⊢ e1 * e2 ⇓ n


            Σ; Δ ⊢ e1 ⇓ n1
            Σ; Δ ⊢ e2 ⇓ n2
            n2 ≠ 0
            n1 mod n2 = 0
            n = n1 / n2
            ─────────────────────────────── [Eval-Div]
            Σ; Δ ⊢ e1 / e2 ⇓ n
```

There is intentionally no rule deriving `Σ; Δ ⊢ p ⇓ n`.

### 3.2 Literal Fit, Refinements, and Patterns

Integer-fit checking is local and straightforward.

```text
            0 ≤ v ≤ 2^w - 1
            κ is unsigned w-bit
            ─────────────────────────────── [Fits-Unsigned]
            Σ ⊢ v fits κ


            -2^(w-1) ≤ v ≤ 2^(w-1) - 1
            κ is signed w-bit
            ─────────────────────────────── [Fits-Signed]
            Σ ⊢ v fits κ


            0 ≤ v ≤ 2^64 - 1
            κ ∈ {btc_varint, uleb128}
            ─────────────────────────────── [Fits-VarInt]
            Σ ⊢ v fits κ
```

Basic refinement validation:

```text
            Σ ⊢ v fits κ
            ─────────────────────────────── [R-Int-Single]
            Σ; Δ ⊢ v ok for prim(κ)


            Σ ⊢ v1 fits κ
            Σ ⊢ v2 fits κ
            v1 ≤ v2
            ─────────────────────────────── [R-Int-Range]
            Σ; Δ ⊢ v1..v2 ok for prim(κ)


            Σ ⊢ v fits κ
            ─────────────────────────────── [R-Int-Prefix]
            Σ; Δ ⊢ ..v ok for prim(κ)


            Σ ⊢ v fits κ
            ─────────────────────────────── [R-Int-Suffix]
            Σ; Δ ⊢ v.. ok for prim(κ)


            ∀i. Σ; Δ ⊢ χi ok for prim(κ)
            ─────────────────────────────── [R-Int-Set]
            Σ; Δ ⊢ {χ1, ..., χn} ok for prim(κ)


            Σ; Δ ⊢ χ ok for prim(κ)
            ─────────────────────────────── [R-Int-Neg]
            Σ; Δ ⊢ !χ ok for prim(κ)
```

Enum refinements are checked against the declared variant set.

```text
            Σ(F) = enum_sig(κ, openness, V̄)
            V ∈ V̄
            ─────────────────────────────── [R-Enum-Single]
            Σ; Δ ⊢ V ok for enum(F)


            Σ(F) = enum_sig(κ, openness, V̄)
            ∀i. Vi ∈ V̄
            ─────────────────────────────── [R-Enum-Set]
            Σ; Δ ⊢ {V1, ..., Vn} ok for enum(F)


            Σ; Δ ⊢ η ok for enum(F)
            ─────────────────────────────── [R-Enum-Neg]
            Σ; Δ ⊢ !η ok for enum(F)
```

Unified branch-pattern typing:

```text
            ─────────────────────────────── [Pat-Wild]
            Σ; Δ ⊢ _ ok for δ


            Σ; Δ ⊢ χ ok for prim(κ)
            ─────────────────────────────── [Pat-Int]
            Σ; Δ ⊢ χ ok for prim(κ)


            Σ; Δ ⊢ η ok for enum(F)
            ─────────────────────────────── [Pat-Enum]
            Σ; Δ ⊢ η ok for enum(F)


            ∀i. Σ ⊢ vi fits u8
            |[v1, ..., vn]| = n
            ─────────────────────────────── [Pat-ByteArray]
            Σ; Δ ⊢ [v1, ..., vn] ok for array(prim(u8), n)


            ∀i. Σ ⊢ vi fits u8
            ─────────────────────────────── [Pat-Bytes]
            Σ; Δ ⊢ [v1, ..., vn] ok for bytes


            ─────────────────────────────── [Pat-Label]
            Σ; Δ ⊢ L ok for •
```

### 3.3 Primitive, Reference, and Container Rules

```text
            ─────────────────────────────── [T-Prim]
            Σ; Δ ⊢ κ : prim(κ)


            host_tyΣ(F) = τ
            ─────────────────────────────── [T-Ref]
            Σ; Δ ⊢ F : τ


            Σ(F) = fmt(Π, τ)
            Π = @x1 : τ1, ..., @xn : τn
            ∀i. @ai : τi ∈ Δ
            ─────────────────────────────── [T-Call]
            Σ; Δ ⊢ F(@a1, ..., @an) : τ


            Σ; Δ ⊢ f : τ
            Σ; Δ ⊢ ψ ok for τ
            ─────────────────────────────── [T-Refine]
            Σ; Δ ⊢ f | ψ : τ


            Σ; Δ ⊢ f : prim(u8)
            Σ; Δ ⊢ e : nat
            Σ; Δ ⊢ e ⇓ n
            ─────────────────────────────── [T-ByteArray]
            Σ; Δ ⊢ [f; e] : array(prim(u8), n)


            Σ; Δ ⊢ f : prim(u8)
            Σ; Δ ⊢ e : nat
            Σ; Δ ⊬ e ⇓ n
            ─────────────────────────────── [T-Bytes]
            Σ; Δ ⊢ [f; e] : bytes


            Σ; Δ ⊢ f : τ
            τ ≠ prim(u8)
            Σ; Δ ⊢ e : nat
            Σ; Δ ⊢ e ⇓ n
            ─────────────────────────────── [T-Array]
            Σ; Δ ⊢ [f; e] : array(τ, n)


            Σ; Δ ⊢ f : τ
            τ ≠ prim(u8)
            Σ; Δ ⊢ e : nat
            Σ; Δ ⊬ e ⇓ n
            ─────────────────────────────── [T-Vec-From-Array]
            Σ; Δ ⊢ [f; e] : vec(τ)


            Σ; Δ ⊢ f : τ
            ─────────────────────────────── [T-Vec]
            Σ; Δ ⊢ Vec<f> : vec(τ)


            Σ; Δ ⊢ f : τ
            ─────────────────────────────── [T-Option]
            Σ; Δ ⊢ Option<f> : option(τ)


            ─────────────────────────────── [T-Tail]
            Σ; Δ ⊢ tail : bytes
```

### 3.4 Sequential Binders and Structs

Field checking is the same left-to-right discipline everywhere: type a binder,
extend `Δ` only when the binder is dependent, then continue with the rest.

```text
            Σ; Δ ⊢ f : τ
            ─────────────────────────────── [B-Dep]
            Σ; Δ ⊢ @x : f ⇒ Δ, @x : τ ; x : τ


            Σ; Δ ⊢ f : τ
            ─────────────────────────────── [B-Ord]
            Σ; Δ ⊢ x : f ⇒ Δ ; x : τ


            Σ ⊢ v : τc
            ─────────────────────────────── [B-Const]
            Σ; Δ ⊢ const x : τc = v ⇒ Δ ; const x : τc
```

```text
            Σ; Δ0 ⊢ b1 ⇒ Δ1 ; ρ1
            Σ; Δ1 ⊢ b2 ⇒ Δ2 ; ρ2
            ...
            Σ; Δn-1 ⊢ bn ⇒ Δn ; ρn
            distinct(id(b1), ..., id(bn))
            ─────────────────────────────── [T-Struct]
            Σ; Δ0 ⊢ { b1, ..., bn } : struct(ρ1, ..., ρn)
```

Here `id(@x : f) = x`, `id(x : f) = x`, and `id(const x : τc = v) = x`.

`wrap` uses the same sequencing idea, except that its result type is the
payload type rather than a record type.

```text
            ∀i. Σ ⊢ ki : τci
            Σ; Δ ⊢ f : τ
            ∀j. Σ ⊢ kj : τcj
            ─────────────────────────────── [T-Wrap]
            Σ; Δ ⊢ wrap(k̄pre, f, k̄post) : τ
```

### 3.5 Choice, Exhaustiveness, and Reinterpretation

The choice rules are consolidated around a single discriminant judgment.

```text
            ─────────────────────────────── [D-None]
            Σ; Δ ⊢ • : •


            @x : prim(κ) ∈ Δ
            ─────────────────────────────── [D-Prim]
            Σ; Δ ⊢ @x : prim(κ)


            @x : enum(F) ∈ Δ
            ─────────────────────────────── [D-Enum]
            Σ; Δ ⊢ @x : enum(F)


            @x : bytes ∈ Δ
            ─────────────────────────────── [D-Bytes]
            Σ; Δ ⊢ @x : bytes


            @x : array(prim(u8), n) ∈ Δ
            ─────────────────────────────── [D-ByteArray]
            Σ; Δ ⊢ @x : array(prim(u8), n)
```

Lightweight exhaustiveness checks:

```text
            ─────────────────────────────── [Exh-None]
            Σ; Δ ⊢ β̄ exhaustive for •


            _ ∈ patterns(β̄)
            ─────────────────────────────── [Exh-Int]
            Σ; Δ ⊢ β̄ exhaustive for prim(κ)


            _ ∈ patterns(β̄)
            ─────────────────────────────── [Exh-Bytes]
            Σ; Δ ⊢ β̄ exhaustive for bytes


            _ ∈ patterns(β̄)
            ─────────────────────────────── [Exh-ByteArray]
            Σ; Δ ⊢ β̄ exhaustive for array(prim(u8), n)


            Σ(F) = enum_sig(κ, closed, V̄)
            covered_variants(β̄) = V̄
            ─────────────────────────────── [Exh-Enum-Closed]
            Σ; Δ ⊢ β̄ exhaustive for enum(F)


            Σ(F) = enum_sig(κ, openness, V̄)
            _ ∈ patterns(β̄)
            ─────────────────────────────── [Exh-Enum-Wild]
            Σ; Δ ⊢ β̄ exhaustive for enum(F)
```

Static-size equality across branches:

```text
            ∀i. Σ ⊢ τi ⇓size n
            ─────────────────────────────── [SizeEq-AllStatic]
            Σ ⊢ (π1 ↦ τ1, ..., πn ↦ τn) same_size


            ∃i. Σ ⊬ τi ⇓size _
            ─────────────────────────────── [SizeEq-Unknown]
            Σ ⊢ (π1 ↦ τ1, ..., πn ↦ τn) same_size
```

Choice typing:

```text
            Σ; Δ ⊢ s : δ
            ∀i. Σ; Δ ⊢ πi ok for δ
            ∀i. Σ; Δ ⊢ fi : τi
            Σ; Δ ⊢ (π1 ↦ τ1, ..., πn ↦ τn) exhaustive for δ
            Σ ⊢ (π1 ↦ τ1, ..., πn ↦ τn) same_size
            ─────────────────────────────── [T-Choice]
            Σ; Δ ⊢ choose(s) { π1 => f1, ..., πn => fn }
                  : choice(δ, π1 ↦ τ1, ..., πn ↦ τn)
```

`choice(δ, β̄)` is a structural precursor of a Rust enum. When the patterns are
enum variants or explicit labels, those names are preserved as Rust variant
names. Other choices may need generated variant names during elaboration; that
naming policy is outside the typing theory.

Reinterpretation is the only place where raw-byte shape matters.

```text
            ─────────────────────────────── [Raw-Bytes]
            Σ ⊢ bytes raw


            ─────────────────────────────── [Raw-ByteArray]
            Σ ⊢ array(prim(u8), n) raw
```

```text
            Σ; Δ ⊢ f1 : τ1
            Σ ⊢ τ1 raw
            Σ; Δ ⊢ f2 : τ2
            ─────────────────────────────── [T-Bind]
            Σ; Δ ⊢ f1 >>= f2 : τ2
```

### 3.6 Constants, Enum Repr Inference, and Exact Static Sizes

Constant typing:

```text
            Σ ⊢ n fits κ
            ─────────────────────────────── [C-Int]
            Σ ⊢ n : prim(κ)


            Σ(F) = enum_sig(κ, openness, V̄)
            V ∈ V̄
            ─────────────────────────────── [C-Enum]
            Σ ⊢ V : enum(F)


            |[v1, ..., vn]| = n
            ∀i. Σ ⊢ vi fits u8
            ─────────────────────────────── [C-Bytes]
            Σ ⊢ [v1, ..., vn] : array(prim(u8), n)
```

Enum representation inference:

```text
            κ = smallest_repr(n̄)
            ∀i. Σ ⊢ ni fits κ
            ─────────────────────────────── [Enum-Repr]
            Σ ⊢ n̄ ⇒repr κ
```

Exact static-size computation:

```text
            size_of_prim(κ) = n
            ─────────────────────────────── [Size-Prim]
            Σ ⊢ prim(κ) ⇓size n


            Σ(F) = enum_sig(κ, openness, V̄)
            size_of_prim(κ) = n
            ─────────────────────────────── [Size-Enum]
            Σ ⊢ enum(F) ⇓size n


            Σ ⊢ τ ⇓size m
            ─────────────────────────────── [Size-Array]
            Σ ⊢ array(τ, n) ⇓size n * m


            ∀i. Σ ⊢ ρi ⇓size ni
            m = Σi ni
            ─────────────────────────────── [Size-Struct]
            Σ ⊢ struct(ρ1, ..., ρk) ⇓size m
```

where record-entry size is:

```text
            Σ ⊢ τ ⇓size n
            ─────────────────────────────── [Size-Field]
            Σ ⊢ (x : τ) ⇓size n


            Σ ⊢ τc ⇓size n
            ─────────────────────────────── [Size-ConstField]
            Σ ⊢ (const x : τc) ⇓size n
```

There are no exact-size rules for `bytes`, `vec`, `option`, or `choice`.

## 4. Definitions and Programs

```text
            F ∉ dom(Σ)
            Σ; ∅ ⊢ f : τ
            ─────────────────────────────── [Def-Fmt-Nullary]
            Σ ⊢ F = f
                  ok ⇒ Σ, F : fmt(·, τ)


            F ∉ dom(Σ)
            Σ ⊢ λ ⇒ Π
            Σ; ΔΠ ⊢ f : τ
            ─────────────────────────────── [Def-Fmt-Param]
            Σ ⊢ F(λ) = f
                  ok ⇒ Σ, F : fmt(Π, τ)


            F ∉ dom(Σ)
            Σ ⊢ v : τc
            ─────────────────────────────── [Def-Const]
            Σ ⊢ const F : τc = v
                  ok ⇒ Σ, F : const(τc)


            F ∉ dom(Σ)
            distinct(V1, ..., Vm)
            Σ ⊢ {n1, ..., nm} ⇒repr κ
            ─────────────────────────────── [Def-Enum]
            Σ ⊢ F = enum[openness] { V1 = n1, ..., Vm = nm }
                  ok ⇒ Σ, F : enum_sig(κ, openness, {V1, ..., Vm})
```

Programs are checked left to right:

```text
            Σ0 = builtins
            Σ0 ⊢ d1 ok ⇒ Σ1
            Σ1 ⊢ d2 ok ⇒ Σ2
            ...
            Σn-1 ⊢ dn ok ⇒ Σn
            ─────────────────────────────── [Prog]
            ⊢ d1 ; ... ; dn ok
```

`builtins` fixes the primitive carriers from Section 1.1.

At the top level, structural results are reified into Rust nominal items:

- if `Σ; ∅ ⊢ f : struct(ρ̄)`, then `F = f` induces a Rust `struct F { ... }`
- if `Σ; ∅ ⊢ f : choice(δ, β̄)`, then `F = f` induces a Rust `enum F { ... }`
- `F = enum { ... }` already induces a Rust `enum F`

Examples of the intended mapping:

- `a = enum { A = 1, B = 2 }` maps to a Rust nominal enum with variants `A` and
  `B`
- `msg(@t : tag) = choose(@t) { A => xxx, B => yyy }` maps to a Rust nominal
  enum with variants `A(Xxx)` and `B(Yyy)` after elaboration of `xxx` and `yyy`
- `tail` and `[u8; @len]` map to `bytes`, that is, Rust `&[u8]`
- `[u8; 32]` maps to `array(prim(u8), 32)`, that is, Rust `[u8; 32]`

## 5. Surface Syntax to Core

The core above is smaller than the surface DSL. The main consolidations are:

| Surface form | Core account |
| --- | --- |
| `u8 | 1..10` and `MyEnum | {A, B}` | both use the single refinement judgment `f \| ψ` |
| `[u8; 32]` | elaborates to `array(prim(u8), 32)` |
| `[u8; @len]` and `tail` | elaborate to `bytes` |
| `[f; e]` for non-byte `f` | elaborates to `array(τ, n)` when `e ⇓ n`, otherwise `vec(τ)` |
| `{ ... }` with ordinary, dependent, and const fields | one left-to-right binder discipline plus a distinct-field check |
| `wrap(pre..., payload, post...)` | same sequencing discipline as structs, but result type is the payload type |
| `choose(@t) { ... }` and `choose { Label(...) }` | one `choose(s)` rule, plus separate local checks for exhaustiveness and branch-size equality |
| explicit enum definitions | use source syntax `F = enum[openness] { ... }` and infer the representation locally |
| parameter annotations | stay in source format syntax `@x : f`, then elaborate to host parameter types |

This is the main consolidation: the typing theory does not need separate rules
for every surface construct once those constructs are factored through the same
core notions of host type, binder, exact static length, raw bytes, and local
validation.

## 6. Later Checks

The following still sit outside this document:

- branch overlap and full parsing unambiguity
- cyclic definitions and recursion policies
- malleability and semantic equivalence properties
- whole-program resource bounds
- any proof obligations that require smt reasoning

Those checks may still exist in the implementation, but they should be modeled
as separate validation layers rather than mixed into the core typing theory.
