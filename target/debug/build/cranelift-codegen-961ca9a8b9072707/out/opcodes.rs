/// An instruction format
///
/// Every opcode has a corresponding instruction format
/// which is represented by both the `InstructionFormat`
/// and the `InstructionData` enums.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum InstructionFormat {
    /// AtomicCas(imms=(flags: ir::MemFlags), vals=3, blocks=0, raw_blocks=0)
    AtomicCas, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// AtomicRmw(imms=(flags: ir::MemFlags, op: ir::AtomicRmwOp), vals=2, blocks=0, raw_blocks=0)
    AtomicRmw, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Binary(imms=(), vals=2, blocks=0, raw_blocks=0)
    Binary, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// BinaryImm64(imms=(imm: ir::immediates::Imm64), vals=1, blocks=0, raw_blocks=0)
    BinaryImm64, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// BinaryImm8(imms=(imm: ir::immediates::Uimm8), vals=1, blocks=0, raw_blocks=0)
    BinaryImm8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// BranchTable(imms=(table: ir::JumpTable), vals=1, blocks=0, raw_blocks=0)
    BranchTable, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Brif(imms=(), vals=1, blocks=2, raw_blocks=0)
    Brif, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Call(imms=(func_ref: ir::FuncRef), vals=0, blocks=0, raw_blocks=0)
    Call, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// CallIndirect(imms=(sig_ref: ir::SigRef), vals=1, blocks=0, raw_blocks=0)
    CallIndirect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// CondTrap(imms=(code: ir::TrapCode), vals=1, blocks=0, raw_blocks=0)
    CondTrap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// DynamicStackLoad(imms=(dynamic_stack_slot: ir::DynamicStackSlot), vals=0, blocks=0, raw_blocks=0)
    DynamicStackLoad, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// DynamicStackStore(imms=(dynamic_stack_slot: ir::DynamicStackSlot), vals=1, blocks=0, raw_blocks=0)
    DynamicStackStore, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// ExceptionHandlerAddress(imms=(imm: ir::immediates::Imm64), vals=0, blocks=0, raw_blocks=1)
    ExceptionHandlerAddress, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// FloatCompare(imms=(cond: ir::condcodes::FloatCC), vals=2, blocks=0, raw_blocks=0)
    FloatCompare, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// FuncAddr(imms=(func_ref: ir::FuncRef), vals=0, blocks=0, raw_blocks=0)
    FuncAddr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// IntAddTrap(imms=(code: ir::TrapCode), vals=2, blocks=0, raw_blocks=0)
    IntAddTrap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// IntCompare(imms=(cond: ir::condcodes::IntCC), vals=2, blocks=0, raw_blocks=0)
    IntCompare, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// IntCompareImm(imms=(cond: ir::condcodes::IntCC, imm: ir::immediates::Imm64), vals=1, blocks=0, raw_blocks=0)
    IntCompareImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Jump(imms=(), vals=0, blocks=1, raw_blocks=0)
    Jump, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Load(imms=(flags: ir::MemFlags, offset: ir::immediates::Offset32), vals=1, blocks=0, raw_blocks=0)
    Load, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// LoadNoOffset(imms=(flags: ir::MemFlags), vals=1, blocks=0, raw_blocks=0)
    LoadNoOffset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// MultiAry(imms=(), vals=0, blocks=0, raw_blocks=0)
    MultiAry, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// NullAry(imms=(), vals=0, blocks=0, raw_blocks=0)
    NullAry, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Shuffle(imms=(imm: ir::Immediate), vals=2, blocks=0, raw_blocks=0)
    Shuffle, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// StackLoad(imms=(stack_slot: ir::StackSlot, offset: ir::immediates::Offset32), vals=0, blocks=0, raw_blocks=0)
    StackLoad, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// StackStore(imms=(stack_slot: ir::StackSlot, offset: ir::immediates::Offset32), vals=1, blocks=0, raw_blocks=0)
    StackStore, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Store(imms=(flags: ir::MemFlags, offset: ir::immediates::Offset32), vals=2, blocks=0, raw_blocks=0)
    Store, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// StoreNoOffset(imms=(flags: ir::MemFlags), vals=2, blocks=0, raw_blocks=0)
    StoreNoOffset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Ternary(imms=(), vals=3, blocks=0, raw_blocks=0)
    Ternary, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// TernaryImm8(imms=(imm: ir::immediates::Uimm8), vals=2, blocks=0, raw_blocks=0)
    TernaryImm8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Trap(imms=(code: ir::TrapCode), vals=0, blocks=0, raw_blocks=0)
    Trap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// TryCall(imms=(func_ref: ir::FuncRef, exception: ir::ExceptionTable), vals=0, blocks=0, raw_blocks=0)
    TryCall, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// TryCallIndirect(imms=(exception: ir::ExceptionTable), vals=1, blocks=0, raw_blocks=0)
    TryCallIndirect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// Unary(imms=(), vals=1, blocks=0, raw_blocks=0)
    Unary, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// UnaryConst(imms=(constant_handle: ir::Constant), vals=0, blocks=0, raw_blocks=0)
    UnaryConst, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// UnaryGlobalValue(imms=(global_value: ir::GlobalValue), vals=0, blocks=0, raw_blocks=0)
    UnaryGlobalValue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// UnaryIeee16(imms=(imm: ir::immediates::Ieee16), vals=0, blocks=0, raw_blocks=0)
    UnaryIeee16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// UnaryIeee32(imms=(imm: ir::immediates::Ieee32), vals=0, blocks=0, raw_blocks=0)
    UnaryIeee32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// UnaryIeee64(imms=(imm: ir::immediates::Ieee64), vals=0, blocks=0, raw_blocks=0)
    UnaryIeee64, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
    /// UnaryImm(imms=(imm: ir::immediates::Imm64), vals=0, blocks=0, raw_blocks=0)
    UnaryImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:32
}

impl<'a> From<&'a InstructionData> for InstructionFormat {
    fn from(inst: &'a InstructionData) -> Self {
        match *inst {
            InstructionData::AtomicCas { .. } => {
                Self::AtomicCas
            }
            InstructionData::AtomicRmw { .. } => {
                Self::AtomicRmw
            }
            InstructionData::Binary { .. } => {
                Self::Binary
            }
            InstructionData::BinaryImm64 { .. } => {
                Self::BinaryImm64
            }
            InstructionData::BinaryImm8 { .. } => {
                Self::BinaryImm8
            }
            InstructionData::BranchTable { .. } => {
                Self::BranchTable
            }
            InstructionData::Brif { .. } => {
                Self::Brif
            }
            InstructionData::Call { .. } => {
                Self::Call
            }
            InstructionData::CallIndirect { .. } => {
                Self::CallIndirect
            }
            InstructionData::CondTrap { .. } => {
                Self::CondTrap
            }
            InstructionData::DynamicStackLoad { .. } => {
                Self::DynamicStackLoad
            }
            InstructionData::DynamicStackStore { .. } => {
                Self::DynamicStackStore
            }
            InstructionData::ExceptionHandlerAddress { .. } => {
                Self::ExceptionHandlerAddress
            }
            InstructionData::FloatCompare { .. } => {
                Self::FloatCompare
            }
            InstructionData::FuncAddr { .. } => {
                Self::FuncAddr
            }
            InstructionData::IntAddTrap { .. } => {
                Self::IntAddTrap
            }
            InstructionData::IntCompare { .. } => {
                Self::IntCompare
            }
            InstructionData::IntCompareImm { .. } => {
                Self::IntCompareImm
            }
            InstructionData::Jump { .. } => {
                Self::Jump
            }
            InstructionData::Load { .. } => {
                Self::Load
            }
            InstructionData::LoadNoOffset { .. } => {
                Self::LoadNoOffset
            }
            InstructionData::MultiAry { .. } => {
                Self::MultiAry
            }
            InstructionData::NullAry { .. } => {
                Self::NullAry
            }
            InstructionData::Shuffle { .. } => {
                Self::Shuffle
            }
            InstructionData::StackLoad { .. } => {
                Self::StackLoad
            }
            InstructionData::StackStore { .. } => {
                Self::StackStore
            }
            InstructionData::Store { .. } => {
                Self::Store
            }
            InstructionData::StoreNoOffset { .. } => {
                Self::StoreNoOffset
            }
            InstructionData::Ternary { .. } => {
                Self::Ternary
            }
            InstructionData::TernaryImm8 { .. } => {
                Self::TernaryImm8
            }
            InstructionData::Trap { .. } => {
                Self::Trap
            }
            InstructionData::TryCall { .. } => {
                Self::TryCall
            }
            InstructionData::TryCallIndirect { .. } => {
                Self::TryCallIndirect
            }
            InstructionData::Unary { .. } => {
                Self::Unary
            }
            InstructionData::UnaryConst { .. } => {
                Self::UnaryConst
            }
            InstructionData::UnaryGlobalValue { .. } => {
                Self::UnaryGlobalValue
            }
            InstructionData::UnaryIeee16 { .. } => {
                Self::UnaryIeee16
            }
            InstructionData::UnaryIeee32 { .. } => {
                Self::UnaryIeee32
            }
            InstructionData::UnaryIeee64 { .. } => {
                Self::UnaryIeee64
            }
            InstructionData::UnaryImm { .. } => {
                Self::UnaryImm
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[allow(missing_docs, reason = "generated code")]
pub enum InstructionData {
    AtomicCas {
        opcode: Opcode,
        args: [Value; 3], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
        flags: ir::MemFlags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    AtomicRmw {
        opcode: Opcode,
        args: [Value; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
        flags: ir::MemFlags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
        op: ir::AtomicRmwOp, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Binary {
        opcode: Opcode,
        args: [Value; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    BinaryImm64 {
        opcode: Opcode,
        arg: Value,
        imm: ir::immediates::Imm64, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    BinaryImm8 {
        opcode: Opcode,
        arg: Value,
        imm: ir::immediates::Uimm8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    BranchTable {
        opcode: Opcode,
        arg: Value,
        table: ir::JumpTable, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Brif {
        opcode: Opcode,
        arg: Value,
        blocks: [ir::BlockCall; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:82
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Call {
        opcode: Opcode,
        args: ValueList,
        func_ref: ir::FuncRef, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    CallIndirect {
        opcode: Opcode,
        args: ValueList,
        sig_ref: ir::SigRef, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    CondTrap {
        opcode: Opcode,
        arg: Value,
        code: ir::TrapCode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    DynamicStackLoad {
        opcode: Opcode,
        dynamic_stack_slot: ir::DynamicStackSlot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    DynamicStackStore {
        opcode: Opcode,
        arg: Value,
        dynamic_stack_slot: ir::DynamicStackSlot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    ExceptionHandlerAddress {
        opcode: Opcode,
        block: ir::Block,
        imm: ir::immediates::Imm64, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    FloatCompare {
        opcode: Opcode,
        args: [Value; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
        cond: ir::condcodes::FloatCC, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    FuncAddr {
        opcode: Opcode,
        func_ref: ir::FuncRef, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    IntAddTrap {
        opcode: Opcode,
        args: [Value; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
        code: ir::TrapCode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    IntCompare {
        opcode: Opcode,
        args: [Value; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
        cond: ir::condcodes::IntCC, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    IntCompareImm {
        opcode: Opcode,
        arg: Value,
        cond: ir::condcodes::IntCC, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
        imm: ir::immediates::Imm64, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Jump {
        opcode: Opcode,
        destination: ir::BlockCall,
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Load {
        opcode: Opcode,
        arg: Value,
        flags: ir::MemFlags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
        offset: ir::immediates::Offset32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    LoadNoOffset {
        opcode: Opcode,
        arg: Value,
        flags: ir::MemFlags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    MultiAry {
        opcode: Opcode,
        args: ValueList,
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    NullAry {
        opcode: Opcode,
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Shuffle {
        opcode: Opcode,
        args: [Value; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
        imm: ir::Immediate, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    StackLoad {
        opcode: Opcode,
        stack_slot: ir::StackSlot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
        offset: ir::immediates::Offset32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    StackStore {
        opcode: Opcode,
        arg: Value,
        stack_slot: ir::StackSlot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
        offset: ir::immediates::Offset32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Store {
        opcode: Opcode,
        args: [Value; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
        flags: ir::MemFlags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
        offset: ir::immediates::Offset32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    StoreNoOffset {
        opcode: Opcode,
        args: [Value; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
        flags: ir::MemFlags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Ternary {
        opcode: Opcode,
        args: [Value; 3], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    TernaryImm8 {
        opcode: Opcode,
        args: [Value; 2], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:76
        imm: ir::immediates::Uimm8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Trap {
        opcode: Opcode,
        code: ir::TrapCode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    TryCall {
        opcode: Opcode,
        args: ValueList,
        func_ref: ir::FuncRef, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
        exception: ir::ExceptionTable, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    TryCallIndirect {
        opcode: Opcode,
        args: ValueList,
        exception: ir::ExceptionTable, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    Unary {
        opcode: Opcode,
        arg: Value,
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    UnaryConst {
        opcode: Opcode,
        constant_handle: ir::Constant, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    UnaryGlobalValue {
        opcode: Opcode,
        global_value: ir::GlobalValue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    UnaryIeee16 {
        opcode: Opcode,
        imm: ir::immediates::Ieee16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    UnaryIeee32 {
        opcode: Opcode,
        imm: ir::immediates::Ieee32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    UnaryIeee64 {
        opcode: Opcode,
        imm: ir::immediates::Ieee64, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
    UnaryImm {
        opcode: Opcode,
        imm: ir::immediates::Imm64, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:97
    }
    , // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:100
}

impl InstructionData {
    /// Get the opcode of this instruction.
    pub fn opcode(&self) -> Opcode {
        match *self {
            Self::AtomicCas { opcode, .. } |
            Self::AtomicRmw { opcode, .. } |
            Self::Binary { opcode, .. } |
            Self::BinaryImm64 { opcode, .. } |
            Self::BinaryImm8 { opcode, .. } |
            Self::BranchTable { opcode, .. } |
            Self::Brif { opcode, .. } |
            Self::Call { opcode, .. } |
            Self::CallIndirect { opcode, .. } |
            Self::CondTrap { opcode, .. } |
            Self::DynamicStackLoad { opcode, .. } |
            Self::DynamicStackStore { opcode, .. } |
            Self::ExceptionHandlerAddress { opcode, .. } |
            Self::FloatCompare { opcode, .. } |
            Self::FuncAddr { opcode, .. } |
            Self::IntAddTrap { opcode, .. } |
            Self::IntCompare { opcode, .. } |
            Self::IntCompareImm { opcode, .. } |
            Self::Jump { opcode, .. } |
            Self::Load { opcode, .. } |
            Self::LoadNoOffset { opcode, .. } |
            Self::MultiAry { opcode, .. } |
            Self::NullAry { opcode, .. } |
            Self::Shuffle { opcode, .. } |
            Self::StackLoad { opcode, .. } |
            Self::StackStore { opcode, .. } |
            Self::Store { opcode, .. } |
            Self::StoreNoOffset { opcode, .. } |
            Self::Ternary { opcode, .. } |
            Self::TernaryImm8 { opcode, .. } |
            Self::Trap { opcode, .. } |
            Self::TryCall { opcode, .. } |
            Self::TryCallIndirect { opcode, .. } |
            Self::Unary { opcode, .. } |
            Self::UnaryConst { opcode, .. } |
            Self::UnaryGlobalValue { opcode, .. } |
            Self::UnaryIeee16 { opcode, .. } |
            Self::UnaryIeee32 { opcode, .. } |
            Self::UnaryIeee64 { opcode, .. } |
            Self::UnaryImm { opcode, .. } => {
                opcode
            }
        }
    }

    /// Get the controlling type variable operand.
    pub fn typevar_operand(&self, pool: &ir::ValueListPool) -> Option<Value> {
        match *self {
            Self::Call { .. } |
            Self::DynamicStackLoad { .. } |
            Self::ExceptionHandlerAddress { .. } |
            Self::FuncAddr { .. } |
            Self::Jump { .. } |
            Self::MultiAry { .. } |
            Self::NullAry { .. } |
            Self::StackLoad { .. } |
            Self::Trap { .. } |
            Self::TryCall { .. } |
            Self::UnaryConst { .. } |
            Self::UnaryGlobalValue { .. } |
            Self::UnaryIeee16 { .. } |
            Self::UnaryIeee32 { .. } |
            Self::UnaryIeee64 { .. } |
            Self::UnaryImm { .. } => {
                None
            }
            Self::BinaryImm64 { arg, .. } |
            Self::BinaryImm8 { arg, .. } |
            Self::BranchTable { arg, .. } |
            Self::Brif { arg, .. } |
            Self::CondTrap { arg, .. } |
            Self::DynamicStackStore { arg, .. } |
            Self::IntCompareImm { arg, .. } |
            Self::Load { arg, .. } |
            Self::LoadNoOffset { arg, .. } |
            Self::StackStore { arg, .. } |
            Self::Unary { arg, .. } => {
                Some(arg)
            }
            Self::AtomicRmw { args: ref args_arity2, .. } |
            Self::Binary { args: ref args_arity2, .. } |
            Self::FloatCompare { args: ref args_arity2, .. } |
            Self::IntAddTrap { args: ref args_arity2, .. } |
            Self::IntCompare { args: ref args_arity2, .. } |
            Self::Shuffle { args: ref args_arity2, .. } |
            Self::Store { args: ref args_arity2, .. } |
            Self::StoreNoOffset { args: ref args_arity2, .. } |
            Self::TernaryImm8 { args: ref args_arity2, .. } => {
                Some(args_arity2[0])
            }
            Self::Ternary { args: ref args_arity3, .. } => {
                Some(args_arity3[1])
            }
            Self::AtomicCas { args: ref args_arity3, .. } => {
                Some(args_arity3[2])
            }
            Self::CallIndirect { ref args, .. } |
            Self::TryCallIndirect { ref args, .. } => {
                args.get(0, pool)
            }
        }
    }

    /// Get the value arguments to this instruction.
    pub fn arguments<'a>(&'a self, pool: &'a ir::ValueListPool) -> &'a [Value] {
        match *self {
            Self::DynamicStackLoad { .. } |
            Self::ExceptionHandlerAddress { .. } |
            Self::FuncAddr { .. } |
            Self::Jump { .. } |
            Self::NullAry { .. } |
            Self::StackLoad { .. } |
            Self::Trap { .. } |
            Self::UnaryConst { .. } |
            Self::UnaryGlobalValue { .. } |
            Self::UnaryIeee16 { .. } |
            Self::UnaryIeee32 { .. } |
            Self::UnaryIeee64 { .. } |
            Self::UnaryImm { .. } => {
                &[]
            }
            Self::AtomicRmw { args: ref args_arity2, .. } |
            Self::Binary { args: ref args_arity2, .. } |
            Self::FloatCompare { args: ref args_arity2, .. } |
            Self::IntAddTrap { args: ref args_arity2, .. } |
            Self::IntCompare { args: ref args_arity2, .. } |
            Self::Shuffle { args: ref args_arity2, .. } |
            Self::Store { args: ref args_arity2, .. } |
            Self::StoreNoOffset { args: ref args_arity2, .. } |
            Self::TernaryImm8 { args: ref args_arity2, .. } => {
                args_arity2
            }
            Self::AtomicCas { args: ref args_arity3, .. } |
            Self::Ternary { args: ref args_arity3, .. } => {
                args_arity3
            }
            Self::BinaryImm64 { ref arg, .. } |
            Self::BinaryImm8 { ref arg, .. } |
            Self::BranchTable { ref arg, .. } |
            Self::Brif { ref arg, .. } |
            Self::CondTrap { ref arg, .. } |
            Self::DynamicStackStore { ref arg, .. } |
            Self::IntCompareImm { ref arg, .. } |
            Self::Load { ref arg, .. } |
            Self::LoadNoOffset { ref arg, .. } |
            Self::StackStore { ref arg, .. } |
            Self::Unary { ref arg, .. } => {
                core::slice::from_ref(arg)
            }
            Self::Call { ref args, .. } |
            Self::CallIndirect { ref args, .. } |
            Self::MultiAry { ref args, .. } |
            Self::TryCall { ref args, .. } |
            Self::TryCallIndirect { ref args, .. } => {
                args.as_slice(pool)
            }
        }
    }

    /// Get mutable references to the value arguments to this
    /// instruction.
    pub fn arguments_mut<'a>(&'a mut self, pool: &'a mut ir::ValueListPool) -> &'a mut [Value] {
        match *self {
            Self::DynamicStackLoad { .. } |
            Self::ExceptionHandlerAddress { .. } |
            Self::FuncAddr { .. } |
            Self::Jump { .. } |
            Self::NullAry { .. } |
            Self::StackLoad { .. } |
            Self::Trap { .. } |
            Self::UnaryConst { .. } |
            Self::UnaryGlobalValue { .. } |
            Self::UnaryIeee16 { .. } |
            Self::UnaryIeee32 { .. } |
            Self::UnaryIeee64 { .. } |
            Self::UnaryImm { .. } => {
                &mut []
            }
            Self::AtomicRmw { args: ref mut args_arity2, .. } |
            Self::Binary { args: ref mut args_arity2, .. } |
            Self::FloatCompare { args: ref mut args_arity2, .. } |
            Self::IntAddTrap { args: ref mut args_arity2, .. } |
            Self::IntCompare { args: ref mut args_arity2, .. } |
            Self::Shuffle { args: ref mut args_arity2, .. } |
            Self::Store { args: ref mut args_arity2, .. } |
            Self::StoreNoOffset { args: ref mut args_arity2, .. } |
            Self::TernaryImm8 { args: ref mut args_arity2, .. } => {
                args_arity2
            }
            Self::AtomicCas { args: ref mut args_arity3, .. } |
            Self::Ternary { args: ref mut args_arity3, .. } => {
                args_arity3
            }
            Self::BinaryImm64 { ref mut arg, .. } |
            Self::BinaryImm8 { ref mut arg, .. } |
            Self::BranchTable { ref mut arg, .. } |
            Self::Brif { ref mut arg, .. } |
            Self::CondTrap { ref mut arg, .. } |
            Self::DynamicStackStore { ref mut arg, .. } |
            Self::IntCompareImm { ref mut arg, .. } |
            Self::Load { ref mut arg, .. } |
            Self::LoadNoOffset { ref mut arg, .. } |
            Self::StackStore { ref mut arg, .. } |
            Self::Unary { ref mut arg, .. } => {
                core::slice::from_mut(arg)
            }
            Self::Call { ref mut args, .. } |
            Self::CallIndirect { ref mut args, .. } |
            Self::MultiAry { ref mut args, .. } |
            Self::TryCall { ref mut args, .. } |
            Self::TryCallIndirect { ref mut args, .. } => {
                args.as_mut_slice(pool)
            }
        }
    }

    /// Compare two `InstructionData` for equality.
    ///
    /// This operation requires a reference to a `ValueListPool` to
    /// determine if the contents of any `ValueLists` are equal.
    ///
    /// This operation takes a closure that is allowed to map each
    /// argument value to some other value before the instructions
    /// are compared. This allows various forms of canonicalization.
    pub fn eq(&self, other: &Self, pool: &ir::ValueListPool) -> bool {
        if ::core::mem::discriminant(self) != ::core::mem::discriminant(other) {
            return false;
        }
        match (self, other) {
            (&Self::AtomicCas { opcode: ref opcode1, args: ref args1, flags: ref flags1 }, &Self::AtomicCas { opcode: ref opcode2, args: ref args2, flags: ref flags2 }) =>  {
                opcode1 == opcode2
                && flags1 == flags2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::AtomicRmw { opcode: ref opcode1, args: ref args1, flags: ref flags1, op: ref op1 }, &Self::AtomicRmw { opcode: ref opcode2, args: ref args2, flags: ref flags2, op: ref op2 }) =>  {
                opcode1 == opcode2
                && flags1 == flags2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && op1 == op2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::Binary { opcode: ref opcode1, args: ref args1 }, &Self::Binary { opcode: ref opcode2, args: ref args2 }) =>  {
                opcode1 == opcode2
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::BinaryImm64 { opcode: ref opcode1, arg: ref arg1, imm: ref imm1 }, &Self::BinaryImm64 { opcode: ref opcode2, arg: ref arg2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::BinaryImm8 { opcode: ref opcode1, arg: ref arg1, imm: ref imm1 }, &Self::BinaryImm8 { opcode: ref opcode2, arg: ref arg2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::BranchTable { opcode: ref opcode1, arg: ref arg1, table: ref table1 }, &Self::BranchTable { opcode: ref opcode2, arg: ref arg2, table: ref table2 }) =>  {
                opcode1 == opcode2
                && table1 == table2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::Brif { opcode: ref opcode1, arg: ref arg1, blocks: ref blocks1 }, &Self::Brif { opcode: ref opcode2, arg: ref arg2, blocks: ref blocks2 }) =>  {
                opcode1 == opcode2
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
                && blocks1.iter().zip(blocks2.iter()).all(|(a, b)| a.block(pool) == b.block(pool)) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:282
            }
            (&Self::Call { opcode: ref opcode1, args: ref args1, func_ref: ref func_ref1 }, &Self::Call { opcode: ref opcode2, args: ref args2, func_ref: ref func_ref2 }) =>  {
                opcode1 == opcode2
                && func_ref1 == func_ref2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.as_slice(pool).iter().zip(args2.as_slice(pool).iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::CallIndirect { opcode: ref opcode1, args: ref args1, sig_ref: ref sig_ref1 }, &Self::CallIndirect { opcode: ref opcode2, args: ref args2, sig_ref: ref sig_ref2 }) =>  {
                opcode1 == opcode2
                && sig_ref1 == sig_ref2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.as_slice(pool).iter().zip(args2.as_slice(pool).iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::CondTrap { opcode: ref opcode1, arg: ref arg1, code: ref code1 }, &Self::CondTrap { opcode: ref opcode2, arg: ref arg2, code: ref code2 }) =>  {
                opcode1 == opcode2
                && code1 == code2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::DynamicStackLoad { opcode: ref opcode1, dynamic_stack_slot: ref dynamic_stack_slot1 }, &Self::DynamicStackLoad { opcode: ref opcode2, dynamic_stack_slot: ref dynamic_stack_slot2 }) =>  {
                opcode1 == opcode2
                && dynamic_stack_slot1 == dynamic_stack_slot2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            (&Self::DynamicStackStore { opcode: ref opcode1, arg: ref arg1, dynamic_stack_slot: ref dynamic_stack_slot1 }, &Self::DynamicStackStore { opcode: ref opcode2, arg: ref arg2, dynamic_stack_slot: ref dynamic_stack_slot2 }) =>  {
                opcode1 == opcode2
                && dynamic_stack_slot1 == dynamic_stack_slot2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::ExceptionHandlerAddress { opcode: ref opcode1, block: ref block1, imm: ref imm1 }, &Self::ExceptionHandlerAddress { opcode: ref opcode2, block: ref block2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && block1 == block2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:285
            }
            (&Self::FloatCompare { opcode: ref opcode1, args: ref args1, cond: ref cond1 }, &Self::FloatCompare { opcode: ref opcode2, args: ref args2, cond: ref cond2 }) =>  {
                opcode1 == opcode2
                && cond1 == cond2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::FuncAddr { opcode: ref opcode1, func_ref: ref func_ref1 }, &Self::FuncAddr { opcode: ref opcode2, func_ref: ref func_ref2 }) =>  {
                opcode1 == opcode2
                && func_ref1 == func_ref2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            (&Self::IntAddTrap { opcode: ref opcode1, args: ref args1, code: ref code1 }, &Self::IntAddTrap { opcode: ref opcode2, args: ref args2, code: ref code2 }) =>  {
                opcode1 == opcode2
                && code1 == code2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::IntCompare { opcode: ref opcode1, args: ref args1, cond: ref cond1 }, &Self::IntCompare { opcode: ref opcode2, args: ref args2, cond: ref cond2 }) =>  {
                opcode1 == opcode2
                && cond1 == cond2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::IntCompareImm { opcode: ref opcode1, arg: ref arg1, cond: ref cond1, imm: ref imm1 }, &Self::IntCompareImm { opcode: ref opcode2, arg: ref arg2, cond: ref cond2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && cond1 == cond2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::Jump { opcode: ref opcode1, destination: ref destination1 }, &Self::Jump { opcode: ref opcode2, destination: ref destination2 }) =>  {
                opcode1 == opcode2
                && destination1 == destination2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:282
            }
            (&Self::Load { opcode: ref opcode1, arg: ref arg1, flags: ref flags1, offset: ref offset1 }, &Self::Load { opcode: ref opcode2, arg: ref arg2, flags: ref flags2, offset: ref offset2 }) =>  {
                opcode1 == opcode2
                && flags1 == flags2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && offset1 == offset2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::LoadNoOffset { opcode: ref opcode1, arg: ref arg1, flags: ref flags1 }, &Self::LoadNoOffset { opcode: ref opcode2, arg: ref arg2, flags: ref flags2 }) =>  {
                opcode1 == opcode2
                && flags1 == flags2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::MultiAry { opcode: ref opcode1, args: ref args1 }, &Self::MultiAry { opcode: ref opcode2, args: ref args2 }) =>  {
                opcode1 == opcode2
                && args1.as_slice(pool).iter().zip(args2.as_slice(pool).iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::NullAry { opcode: ref opcode1 }, &Self::NullAry { opcode: ref opcode2 }) =>  {
                opcode1 == opcode2
            }
            (&Self::Shuffle { opcode: ref opcode1, args: ref args1, imm: ref imm1 }, &Self::Shuffle { opcode: ref opcode2, args: ref args2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::StackLoad { opcode: ref opcode1, stack_slot: ref stack_slot1, offset: ref offset1 }, &Self::StackLoad { opcode: ref opcode2, stack_slot: ref stack_slot2, offset: ref offset2 }) =>  {
                opcode1 == opcode2
                && stack_slot1 == stack_slot2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && offset1 == offset2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            (&Self::StackStore { opcode: ref opcode1, arg: ref arg1, stack_slot: ref stack_slot1, offset: ref offset1 }, &Self::StackStore { opcode: ref opcode2, arg: ref arg2, stack_slot: ref stack_slot2, offset: ref offset2 }) =>  {
                opcode1 == opcode2
                && stack_slot1 == stack_slot2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && offset1 == offset2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::Store { opcode: ref opcode1, args: ref args1, flags: ref flags1, offset: ref offset1 }, &Self::Store { opcode: ref opcode2, args: ref args2, flags: ref flags2, offset: ref offset2 }) =>  {
                opcode1 == opcode2
                && flags1 == flags2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && offset1 == offset2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::StoreNoOffset { opcode: ref opcode1, args: ref args1, flags: ref flags1 }, &Self::StoreNoOffset { opcode: ref opcode2, args: ref args2, flags: ref flags2 }) =>  {
                opcode1 == opcode2
                && flags1 == flags2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::Ternary { opcode: ref opcode1, args: ref args1 }, &Self::Ternary { opcode: ref opcode2, args: ref args2 }) =>  {
                opcode1 == opcode2
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::TernaryImm8 { opcode: ref opcode1, args: ref args1, imm: ref imm1 }, &Self::TernaryImm8 { opcode: ref opcode2, args: ref args2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.iter().zip(args2.iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::Trap { opcode: ref opcode1, code: ref code1 }, &Self::Trap { opcode: ref opcode2, code: ref code2 }) =>  {
                opcode1 == opcode2
                && code1 == code2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            (&Self::TryCall { opcode: ref opcode1, args: ref args1, func_ref: ref func_ref1, exception: ref exception1 }, &Self::TryCall { opcode: ref opcode2, args: ref args2, func_ref: ref func_ref2, exception: ref exception2 }) =>  {
                opcode1 == opcode2
                && func_ref1 == func_ref2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && exception1 == exception2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.as_slice(pool).iter().zip(args2.as_slice(pool).iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::TryCallIndirect { opcode: ref opcode1, args: ref args1, exception: ref exception1 }, &Self::TryCallIndirect { opcode: ref opcode2, args: ref args2, exception: ref exception2 }) =>  {
                opcode1 == opcode2
                && exception1 == exception2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
                && args1.as_slice(pool).iter().zip(args2.as_slice(pool).iter()).all(|(a, b)| a == b) // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::Unary { opcode: ref opcode1, arg: ref arg1 }, &Self::Unary { opcode: ref opcode2, arg: ref arg2 }) =>  {
                opcode1 == opcode2
                && arg1 == arg2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:279
            }
            (&Self::UnaryConst { opcode: ref opcode1, constant_handle: ref constant_handle1 }, &Self::UnaryConst { opcode: ref opcode2, constant_handle: ref constant_handle2 }) =>  {
                opcode1 == opcode2
                && constant_handle1 == constant_handle2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            (&Self::UnaryGlobalValue { opcode: ref opcode1, global_value: ref global_value1 }, &Self::UnaryGlobalValue { opcode: ref opcode2, global_value: ref global_value2 }) =>  {
                opcode1 == opcode2
                && global_value1 == global_value2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            (&Self::UnaryIeee16 { opcode: ref opcode1, imm: ref imm1 }, &Self::UnaryIeee16 { opcode: ref opcode2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            (&Self::UnaryIeee32 { opcode: ref opcode1, imm: ref imm1 }, &Self::UnaryIeee32 { opcode: ref opcode2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            (&Self::UnaryIeee64 { opcode: ref opcode1, imm: ref imm1 }, &Self::UnaryIeee64 { opcode: ref opcode2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            (&Self::UnaryImm { opcode: ref opcode1, imm: ref imm1 }, &Self::UnaryImm { opcode: ref opcode2, imm: ref imm2 }) =>  {
                opcode1 == opcode2
                && imm1 == imm2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:276
            }
            _ => unreachable!()
        }
    }

    /// Hash an `InstructionData`.
    ///
    /// This operation requires a reference to a `ValueListPool` to
    /// hash the contents of any `ValueLists`.
    ///
    /// This operation takes a closure that is allowed to map each
    /// argument value to some other value before it is hashed. This
    /// allows various forms of canonicalization.
    pub fn hash<H: ::core::hash::Hasher>(&self, state: &mut H, pool: &ir::ValueListPool) {
        match *self {
            Self::AtomicCas{opcode, ref args, flags} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&flags, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::AtomicRmw{opcode, ref args, flags, op} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&flags, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&op, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::Binary{opcode, ref args} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::BinaryImm64{opcode, ref arg, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::BinaryImm8{opcode, ref arg, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::BranchTable{opcode, ref arg, table} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&table, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::Brif{opcode, ref arg, ref blocks} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
                ::core::hash::Hash::hash(&blocks.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:363
                for &block in blocks {
                    ::core::hash::Hash::hash(&block.block(pool), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:365
                    for arg in block.args(pool) {
                        ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:367
                    }
                }
            }
            Self::Call{opcode, ref args, func_ref} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&func_ref, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(pool), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args.as_slice(pool) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::CallIndirect{opcode, ref args, sig_ref} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&sig_ref, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(pool), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args.as_slice(pool) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::CondTrap{opcode, ref arg, code} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&code, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::DynamicStackLoad{opcode, dynamic_stack_slot} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&dynamic_stack_slot, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::DynamicStackStore{opcode, ref arg, dynamic_stack_slot} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&dynamic_stack_slot, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::ExceptionHandlerAddress{opcode, block, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                ::core::hash::Hash::hash(&block, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:373
            }
            Self::FloatCompare{opcode, ref args, cond} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&cond, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::FuncAddr{opcode, func_ref} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&func_ref, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::IntAddTrap{opcode, ref args, code} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&code, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::IntCompare{opcode, ref args, cond} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&cond, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::IntCompareImm{opcode, ref arg, cond, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&cond, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::Jump{opcode, ref destination} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:363
                for &block in std::slice::from_ref(destination) {
                    ::core::hash::Hash::hash(&block.block(pool), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:365
                    for arg in block.args(pool) {
                        ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:367
                    }
                }
            }
            Self::Load{opcode, ref arg, flags, offset} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&flags, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&offset, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::LoadNoOffset{opcode, ref arg, flags} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&flags, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::MultiAry{opcode, ref args} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&args.len(pool), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args.as_slice(pool) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::NullAry{opcode} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::Shuffle{opcode, ref args, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::StackLoad{opcode, stack_slot, offset} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&stack_slot, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&offset, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::StackStore{opcode, ref arg, stack_slot, offset} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&stack_slot, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&offset, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::Store{opcode, ref args, flags, offset} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&flags, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&offset, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::StoreNoOffset{opcode, ref args, flags} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&flags, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::Ternary{opcode, ref args} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::TernaryImm8{opcode, ref args, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::Trap{opcode, code} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&code, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::TryCall{opcode, ref args, func_ref, exception} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&func_ref, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&exception, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(pool), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args.as_slice(pool) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::TryCallIndirect{opcode, ref args, exception} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&exception, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&args.len(pool), state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in args.as_slice(pool) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::Unary{opcode, ref arg} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&1, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
                for &arg in std::slice::from_ref(arg) {
                    ::core::hash::Hash::hash(&arg, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:358
                }
            }
            Self::UnaryConst{opcode, constant_handle} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&constant_handle, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::UnaryGlobalValue{opcode, global_value} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&global_value, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::UnaryIeee16{opcode, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::UnaryIeee32{opcode, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::UnaryIeee64{opcode, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
            Self::UnaryImm{opcode, imm} =>  {
                ::core::hash::Hash::hash( &::core::mem::discriminant(self), state);
                ::core::hash::Hash::hash(&opcode, state);
                ::core::hash::Hash::hash(&imm, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:353
                ::core::hash::Hash::hash(&0, state); // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:355
            }
        }
    }

    /// Deep-clone an `InstructionData`, including any referenced lists.
    ///
    /// This operation requires a reference to a `ValueListPool` to
    /// clone the `ValueLists`.
    pub fn deep_clone(&self, pool: &mut ir::ValueListPool) -> Self {
        match *self {
            Self::AtomicCas{opcode, args, flags} =>  {
                Self::AtomicCas {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::AtomicRmw{opcode, args, flags, op} =>  {
                Self::AtomicRmw {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                    op, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::Binary{opcode, args} =>  {
                Self::Binary {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                }
            }
            Self::BinaryImm64{opcode, arg, imm} =>  {
                Self::BinaryImm64 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::BinaryImm8{opcode, arg, imm} =>  {
                Self::BinaryImm8 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::BranchTable{opcode, arg, table} =>  {
                Self::BranchTable {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    table, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::Brif{opcode, arg, blocks} =>  {
                Self::Brif {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    blocks: [blocks[0].deep_clone(pool), blocks[1].deep_clone(pool)], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:443
                }
            }
            Self::Call{opcode, ref args, func_ref} =>  {
                Self::Call {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args: args.deep_clone(pool), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:430
                    func_ref, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::CallIndirect{opcode, ref args, sig_ref} =>  {
                Self::CallIndirect {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args: args.deep_clone(pool), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:430
                    sig_ref, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::CondTrap{opcode, arg, code} =>  {
                Self::CondTrap {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    code, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::DynamicStackLoad{opcode, dynamic_stack_slot} =>  {
                Self::DynamicStackLoad {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    dynamic_stack_slot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::DynamicStackStore{opcode, arg, dynamic_stack_slot} =>  {
                Self::DynamicStackStore {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    dynamic_stack_slot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::ExceptionHandlerAddress{opcode, block, imm} =>  {
                Self::ExceptionHandlerAddress {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    block, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:451
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::FloatCompare{opcode, args, cond} =>  {
                Self::FloatCompare {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                    cond, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::FuncAddr{opcode, func_ref} =>  {
                Self::FuncAddr {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    func_ref, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::IntAddTrap{opcode, args, code} =>  {
                Self::IntAddTrap {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                    code, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::IntCompare{opcode, args, cond} =>  {
                Self::IntCompare {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                    cond, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::IntCompareImm{opcode, arg, cond, imm} =>  {
                Self::IntCompareImm {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    cond, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::Jump{opcode, destination} =>  {
                Self::Jump {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    destination: destination.deep_clone(pool), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:440
                }
            }
            Self::Load{opcode, arg, flags, offset} =>  {
                Self::Load {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                    offset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::LoadNoOffset{opcode, arg, flags} =>  {
                Self::LoadNoOffset {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::MultiAry{opcode, ref args} =>  {
                Self::MultiAry {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args: args.deep_clone(pool), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:430
                }
            }
            Self::NullAry{opcode} =>  {
                Self::NullAry {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                }
            }
            Self::Shuffle{opcode, args, imm} =>  {
                Self::Shuffle {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::StackLoad{opcode, stack_slot, offset} =>  {
                Self::StackLoad {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    stack_slot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                    offset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::StackStore{opcode, arg, stack_slot, offset} =>  {
                Self::StackStore {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                    stack_slot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                    offset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::Store{opcode, args, flags, offset} =>  {
                Self::Store {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                    offset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::StoreNoOffset{opcode, args, flags} =>  {
                Self::StoreNoOffset {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::Ternary{opcode, args} =>  {
                Self::Ternary {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                }
            }
            Self::TernaryImm8{opcode, args, imm} =>  {
                Self::TernaryImm8 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:434
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::Trap{opcode, code} =>  {
                Self::Trap {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    code, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::TryCall{opcode, ref args, func_ref, exception} =>  {
                Self::TryCall {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args: args.deep_clone(pool), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:430
                    func_ref, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                    exception, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::TryCallIndirect{opcode, ref args, exception} =>  {
                Self::TryCallIndirect {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    args: args.deep_clone(pool), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:430
                    exception, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::Unary{opcode, arg} =>  {
                Self::Unary {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    arg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:432
                }
            }
            Self::UnaryConst{opcode, constant_handle} =>  {
                Self::UnaryConst {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    constant_handle, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::UnaryGlobalValue{opcode, global_value} =>  {
                Self::UnaryGlobalValue {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    global_value, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::UnaryIeee16{opcode, imm} =>  {
                Self::UnaryIeee16 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::UnaryIeee32{opcode, imm} =>  {
                Self::UnaryIeee32 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::UnaryIeee64{opcode, imm} =>  {
                Self::UnaryIeee64 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
            Self::UnaryImm{opcode, imm} =>  {
                Self::UnaryImm {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:427
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:457
                }
            }
        }
    }
    /// Map some functions, described by the given `InstructionMapper`, over each of the
    /// entities within this instruction, producing a new `InstructionData`.
    pub fn map(&self, mut mapper: impl crate::ir::instructions::InstructionMapper) -> Self {
        match *self {
            Self::AtomicCas{opcode, args, flags} =>  {
                Self::AtomicCas {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1]), mapper.map_value(args[2])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::AtomicRmw{opcode, args, flags, op} =>  {
                Self::AtomicRmw {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                    op, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::Binary{opcode, args} =>  {
                Self::Binary {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                }
            }
            Self::BinaryImm64{opcode, arg, imm} =>  {
                Self::BinaryImm64 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::BinaryImm8{opcode, arg, imm} =>  {
                Self::BinaryImm8 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::BranchTable{opcode, arg, table} =>  {
                Self::BranchTable {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    table: mapper.map_jump_table(table), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::Brif{opcode, arg, blocks} =>  {
                Self::Brif {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    blocks: [mapper.map_block_call(blocks[0]), mapper.map_block_call(blocks[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:527
                }
            }
            Self::Call{opcode, args, func_ref} =>  {
                Self::Call {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: mapper.map_value_list(args), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:510
                    func_ref: mapper.map_func_ref(func_ref), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::CallIndirect{opcode, args, sig_ref} =>  {
                Self::CallIndirect {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: mapper.map_value_list(args), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:510
                    sig_ref: mapper.map_sig_ref(sig_ref), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::CondTrap{opcode, arg, code} =>  {
                Self::CondTrap {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    code, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::DynamicStackLoad{opcode, dynamic_stack_slot} =>  {
                Self::DynamicStackLoad {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    dynamic_stack_slot: mapper.map_dynamic_stack_slot(dynamic_stack_slot), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::DynamicStackStore{opcode, arg, dynamic_stack_slot} =>  {
                Self::DynamicStackStore {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    dynamic_stack_slot: mapper.map_dynamic_stack_slot(dynamic_stack_slot), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::ExceptionHandlerAddress{opcode, block, imm} =>  {
                Self::ExceptionHandlerAddress {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    block: mapper.map_block(block), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:535
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::FloatCompare{opcode, args, cond} =>  {
                Self::FloatCompare {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                    cond, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::FuncAddr{opcode, func_ref} =>  {
                Self::FuncAddr {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    func_ref: mapper.map_func_ref(func_ref), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::IntAddTrap{opcode, args, code} =>  {
                Self::IntAddTrap {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                    code, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::IntCompare{opcode, args, cond} =>  {
                Self::IntCompare {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                    cond, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::IntCompareImm{opcode, arg, cond, imm} =>  {
                Self::IntCompareImm {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    cond, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::Jump{opcode, destination} =>  {
                Self::Jump {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    destination: mapper.map_block_call(destination), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:524
                }
            }
            Self::Load{opcode, arg, flags, offset} =>  {
                Self::Load {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                    offset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::LoadNoOffset{opcode, arg, flags} =>  {
                Self::LoadNoOffset {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::MultiAry{opcode, args} =>  {
                Self::MultiAry {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: mapper.map_value_list(args), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:510
                }
            }
            Self::NullAry{opcode} =>  {
                Self::NullAry {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                }
            }
            Self::Shuffle{opcode, args, imm} =>  {
                Self::Shuffle {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                    imm: mapper.map_immediate(imm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::StackLoad{opcode, stack_slot, offset} =>  {
                Self::StackLoad {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    stack_slot: mapper.map_stack_slot(stack_slot), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                    offset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::StackStore{opcode, arg, stack_slot, offset} =>  {
                Self::StackStore {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                    stack_slot: mapper.map_stack_slot(stack_slot), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                    offset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::Store{opcode, args, flags, offset} =>  {
                Self::Store {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                    offset, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::StoreNoOffset{opcode, args, flags} =>  {
                Self::StoreNoOffset {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                    flags, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::Ternary{opcode, args} =>  {
                Self::Ternary {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1]), mapper.map_value(args[2])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                }
            }
            Self::TernaryImm8{opcode, args, imm} =>  {
                Self::TernaryImm8 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: [mapper.map_value(args[0]), mapper.map_value(args[1])], // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:518
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::Trap{opcode, code} =>  {
                Self::Trap {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    code, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::TryCall{opcode, args, func_ref, exception} =>  {
                Self::TryCall {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: mapper.map_value_list(args), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:510
                    func_ref: mapper.map_func_ref(func_ref), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                    exception: mapper.map_exception_table(exception), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::TryCallIndirect{opcode, args, exception} =>  {
                Self::TryCallIndirect {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    args: mapper.map_value_list(args), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:510
                    exception: mapper.map_exception_table(exception), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::Unary{opcode, arg} =>  {
                Self::Unary {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    arg: mapper.map_value(arg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:512
                }
            }
            Self::UnaryConst{opcode, constant_handle} =>  {
                Self::UnaryConst {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    constant_handle: mapper.map_constant(constant_handle), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::UnaryGlobalValue{opcode, global_value} =>  {
                Self::UnaryGlobalValue {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    global_value: mapper.map_global_value(global_value), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:555
                }
            }
            Self::UnaryIeee16{opcode, imm} =>  {
                Self::UnaryIeee16 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::UnaryIeee32{opcode, imm} =>  {
                Self::UnaryIeee32 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::UnaryIeee64{opcode, imm} =>  {
                Self::UnaryIeee64 {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
            Self::UnaryImm{opcode, imm} =>  {
                Self::UnaryImm {
                    opcode, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:507
                    imm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:562
                }
            }
        }
    }
}

/// An instruction opcode.
///
/// All instructions from all supported ISAs are present.
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(
            feature = "enable-serde",
            derive(serde_derive::Serialize, serde_derive::Deserialize)
        )]
pub enum Opcode {
    /// `jump block_call`. (Jump)
    Jump = 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:633
    /// `brif c, block_then, block_else`. (Brif)
    /// Type inferred from `c`.
    Brif, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `br_table x, JT`. (BranchTable)
    BrTable, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `debugtrap`. (NullAry)
    Debugtrap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `trap code`. (Trap)
    Trap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `trapz c, code`. (CondTrap)
    /// Type inferred from `c`.
    Trapz, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `trapnz c, code`. (CondTrap)
    /// Type inferred from `c`.
    Trapnz, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `return rvals`. (MultiAry)
    Return, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `rvals = call FN, args`. (Call)
    Call, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `rvals = call_indirect SIG, callee, args`. (CallIndirect)
    /// Type inferred from `callee`.
    CallIndirect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `return_call FN, args`. (Call)
    ReturnCall, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `return_call_indirect SIG, callee, args`. (CallIndirect)
    /// Type inferred from `callee`.
    ReturnCallIndirect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `addr = func_addr FN`. (FuncAddr)
    FuncAddr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `try_call callee, args, ET`. (TryCall)
    TryCall, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `try_call_indirect callee, args, ET`. (TryCallIndirect)
    /// Type inferred from `callee`.
    TryCallIndirect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = splat x`. (Unary)
    Splat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = swizzle x, y`. (Binary)
    Swizzle, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = x86_pshufb x, y`. (Binary)
    X86Pshufb, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = insertlane x, y, Idx`. (TernaryImm8)
    /// Type inferred from `x`.
    Insertlane, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = extractlane x, Idx`. (BinaryImm8)
    /// Type inferred from `x`.
    Extractlane, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = smin x, y`. (Binary)
    /// Type inferred from `x`.
    Smin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = umin x, y`. (Binary)
    /// Type inferred from `x`.
    Umin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = smax x, y`. (Binary)
    /// Type inferred from `x`.
    Smax, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = umax x, y`. (Binary)
    /// Type inferred from `x`.
    Umax, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = avg_round x, y`. (Binary)
    /// Type inferred from `x`.
    AvgRound, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uadd_sat x, y`. (Binary)
    /// Type inferred from `x`.
    UaddSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sadd_sat x, y`. (Binary)
    /// Type inferred from `x`.
    SaddSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = usub_sat x, y`. (Binary)
    /// Type inferred from `x`.
    UsubSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = ssub_sat x, y`. (Binary)
    /// Type inferred from `x`.
    SsubSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = load MemFlags, p, Offset`. (Load)
    Load, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `store MemFlags, x, p, Offset`. (Store)
    /// Type inferred from `x`.
    Store, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uload8 MemFlags, p, Offset`. (Load)
    Uload8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sload8 MemFlags, p, Offset`. (Load)
    Sload8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `istore8 MemFlags, x, p, Offset`. (Store)
    /// Type inferred from `x`.
    Istore8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uload16 MemFlags, p, Offset`. (Load)
    Uload16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sload16 MemFlags, p, Offset`. (Load)
    Sload16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `istore16 MemFlags, x, p, Offset`. (Store)
    /// Type inferred from `x`.
    Istore16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uload32 MemFlags, p, Offset`. (Load)
    /// Type inferred from `p`.
    Uload32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sload32 MemFlags, p, Offset`. (Load)
    /// Type inferred from `p`.
    Sload32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `istore32 MemFlags, x, p, Offset`. (Store)
    /// Type inferred from `x`.
    Istore32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `out_payload0 = stack_switch store_context_ptr, load_context_ptr, in_payload0`. (Ternary)
    /// Type inferred from `load_context_ptr`.
    StackSwitch, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uload8x8 MemFlags, p, Offset`. (Load)
    /// Type inferred from `p`.
    Uload8x8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sload8x8 MemFlags, p, Offset`. (Load)
    /// Type inferred from `p`.
    Sload8x8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uload16x4 MemFlags, p, Offset`. (Load)
    /// Type inferred from `p`.
    Uload16x4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sload16x4 MemFlags, p, Offset`. (Load)
    /// Type inferred from `p`.
    Sload16x4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uload32x2 MemFlags, p, Offset`. (Load)
    /// Type inferred from `p`.
    Uload32x2, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sload32x2 MemFlags, p, Offset`. (Load)
    /// Type inferred from `p`.
    Sload32x2, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = stack_load SS, Offset`. (StackLoad)
    StackLoad, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `stack_store x, SS, Offset`. (StackStore)
    /// Type inferred from `x`.
    StackStore, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `addr = stack_addr SS, Offset`. (StackLoad)
    StackAddr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = dynamic_stack_load DSS`. (DynamicStackLoad)
    DynamicStackLoad, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `dynamic_stack_store x, DSS`. (DynamicStackStore)
    /// Type inferred from `x`.
    DynamicStackStore, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `addr = dynamic_stack_addr DSS`. (DynamicStackLoad)
    DynamicStackAddr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = global_value GV`. (UnaryGlobalValue)
    GlobalValue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = symbol_value GV`. (UnaryGlobalValue)
    SymbolValue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = tls_value GV`. (UnaryGlobalValue)
    TlsValue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `addr = get_pinned_reg`. (NullAry)
    GetPinnedReg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `set_pinned_reg addr`. (Unary)
    /// Type inferred from `addr`.
    SetPinnedReg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `addr = get_frame_pointer`. (NullAry)
    GetFramePointer, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `addr = get_stack_pointer`. (NullAry)
    GetStackPointer, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `addr = get_return_address`. (NullAry)
    GetReturnAddress, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `addr = get_exception_handler_address block, index`. (ExceptionHandlerAddress)
    GetExceptionHandlerAddress, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = iconst N`. (UnaryImm)
    Iconst, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = f16const N`. (UnaryIeee16)
    F16const, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = f32const N`. (UnaryIeee32)
    F32const, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = f64const N`. (UnaryIeee64)
    F64const, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = f128const N`. (UnaryConst)
    F128const, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = vconst N`. (UnaryConst)
    Vconst, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = shuffle a, b, mask`. (Shuffle)
    Shuffle, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `nop`. (NullAry)
    Nop, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = select c, x, y`. (Ternary)
    /// Type inferred from `x`.
    Select, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = select_spectre_guard c, x, y`. (Ternary)
    /// Type inferred from `x`.
    SelectSpectreGuard, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bitselect c, x, y`. (Ternary)
    /// Type inferred from `x`.
    Bitselect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = x86_blendv c, x, y`. (Ternary)
    /// Type inferred from `x`.
    X86Blendv, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `s = vany_true a`. (Unary)
    /// Type inferred from `a`.
    VanyTrue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `s = vall_true a`. (Unary)
    /// Type inferred from `a`.
    VallTrue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `x = vhigh_bits a`. (Unary)
    VhighBits, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = icmp Cond, x, y`. (IntCompare)
    /// Type inferred from `x`.
    Icmp, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = icmp_imm Cond, x, Y`. (IntCompareImm)
    /// Type inferred from `x`.
    IcmpImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = iadd x, y`. (Binary)
    /// Type inferred from `x`.
    Iadd, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = isub x, y`. (Binary)
    /// Type inferred from `x`.
    Isub, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = ineg x`. (Unary)
    /// Type inferred from `x`.
    Ineg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = iabs x`. (Unary)
    /// Type inferred from `x`.
    Iabs, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = imul x, y`. (Binary)
    /// Type inferred from `x`.
    Imul, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = umulhi x, y`. (Binary)
    /// Type inferred from `x`.
    Umulhi, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = smulhi x, y`. (Binary)
    /// Type inferred from `x`.
    Smulhi, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sqmul_round_sat x, y`. (Binary)
    /// Type inferred from `x`.
    SqmulRoundSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = x86_pmulhrsw x, y`. (Binary)
    /// Type inferred from `x`.
    X86Pmulhrsw, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = udiv x, y`. (Binary)
    /// Type inferred from `x`.
    Udiv, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sdiv x, y`. (Binary)
    /// Type inferred from `x`.
    Sdiv, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = urem x, y`. (Binary)
    /// Type inferred from `x`.
    Urem, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = srem x, y`. (Binary)
    /// Type inferred from `x`.
    Srem, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = iadd_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    IaddImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = imul_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    ImulImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = udiv_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    UdivImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sdiv_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    SdivImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = urem_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    UremImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = srem_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    SremImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = irsub_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    IrsubImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, c_out = sadd_overflow_cin x, y, c_in`. (Ternary)
    /// Type inferred from `y`.
    SaddOverflowCin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, c_out = uadd_overflow_cin x, y, c_in`. (Ternary)
    /// Type inferred from `y`.
    UaddOverflowCin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, of = uadd_overflow x, y`. (Binary)
    /// Type inferred from `x`.
    UaddOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, of = sadd_overflow x, y`. (Binary)
    /// Type inferred from `x`.
    SaddOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, of = usub_overflow x, y`. (Binary)
    /// Type inferred from `x`.
    UsubOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, of = ssub_overflow x, y`. (Binary)
    /// Type inferred from `x`.
    SsubOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, of = umul_overflow x, y`. (Binary)
    /// Type inferred from `x`.
    UmulOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, of = smul_overflow x, y`. (Binary)
    /// Type inferred from `x`.
    SmulOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uadd_overflow_trap x, y, code`. (IntAddTrap)
    /// Type inferred from `x`.
    UaddOverflowTrap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, b_out = ssub_overflow_bin x, y, b_in`. (Ternary)
    /// Type inferred from `y`.
    SsubOverflowBin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a, b_out = usub_overflow_bin x, y, b_in`. (Ternary)
    /// Type inferred from `y`.
    UsubOverflowBin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = band x, y`. (Binary)
    /// Type inferred from `x`.
    Band, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bor x, y`. (Binary)
    /// Type inferred from `x`.
    Bor, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bxor x, y`. (Binary)
    /// Type inferred from `x`.
    Bxor, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bnot x`. (Unary)
    /// Type inferred from `x`.
    Bnot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = band_not x, y`. (Binary)
    /// Type inferred from `x`.
    BandNot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bor_not x, y`. (Binary)
    /// Type inferred from `x`.
    BorNot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bxor_not x, y`. (Binary)
    /// Type inferred from `x`.
    BxorNot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = band_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    BandImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bor_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    BorImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bxor_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    BxorImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = rotl x, y`. (Binary)
    /// Type inferred from `x`.
    Rotl, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = rotr x, y`. (Binary)
    /// Type inferred from `x`.
    Rotr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = rotl_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    RotlImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = rotr_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    RotrImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = ishl x, y`. (Binary)
    /// Type inferred from `x`.
    Ishl, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = ushr x, y`. (Binary)
    /// Type inferred from `x`.
    Ushr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sshr x, y`. (Binary)
    /// Type inferred from `x`.
    Sshr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = ishl_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    IshlImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = ushr_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    UshrImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sshr_imm x, Y`. (BinaryImm64)
    /// Type inferred from `x`.
    SshrImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bitrev x`. (Unary)
    /// Type inferred from `x`.
    Bitrev, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = clz x`. (Unary)
    /// Type inferred from `x`.
    Clz, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = cls x`. (Unary)
    /// Type inferred from `x`.
    Cls, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = ctz x`. (Unary)
    /// Type inferred from `x`.
    Ctz, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bswap x`. (Unary)
    /// Type inferred from `x`.
    Bswap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = popcnt x`. (Unary)
    /// Type inferred from `x`.
    Popcnt, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fcmp Cond, x, y`. (FloatCompare)
    /// Type inferred from `x`.
    Fcmp, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fadd x, y`. (Binary)
    /// Type inferred from `x`.
    Fadd, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fsub x, y`. (Binary)
    /// Type inferred from `x`.
    Fsub, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fmul x, y`. (Binary)
    /// Type inferred from `x`.
    Fmul, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fdiv x, y`. (Binary)
    /// Type inferred from `x`.
    Fdiv, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sqrt x`. (Unary)
    /// Type inferred from `x`.
    Sqrt, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fma x, y, z`. (Ternary)
    /// Type inferred from `y`.
    Fma, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fneg x`. (Unary)
    /// Type inferred from `x`.
    Fneg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fabs x`. (Unary)
    /// Type inferred from `x`.
    Fabs, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fcopysign x, y`. (Binary)
    /// Type inferred from `x`.
    Fcopysign, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fmin x, y`. (Binary)
    /// Type inferred from `x`.
    Fmin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fmax x, y`. (Binary)
    /// Type inferred from `x`.
    Fmax, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = ceil x`. (Unary)
    /// Type inferred from `x`.
    Ceil, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = floor x`. (Unary)
    /// Type inferred from `x`.
    Floor, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = trunc x`. (Unary)
    /// Type inferred from `x`.
    Trunc, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = nearest x`. (Unary)
    /// Type inferred from `x`.
    Nearest, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bitcast MemFlags, x`. (LoadNoOffset)
    Bitcast, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = scalar_to_vector s`. (Unary)
    ScalarToVector, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = bmask x`. (Unary)
    Bmask, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = ireduce x`. (Unary)
    Ireduce, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = snarrow x, y`. (Binary)
    /// Type inferred from `x`.
    Snarrow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = unarrow x, y`. (Binary)
    /// Type inferred from `x`.
    Unarrow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uunarrow x, y`. (Binary)
    /// Type inferred from `x`.
    Uunarrow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = swiden_low x`. (Unary)
    /// Type inferred from `x`.
    SwidenLow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = swiden_high x`. (Unary)
    /// Type inferred from `x`.
    SwidenHigh, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uwiden_low x`. (Unary)
    /// Type inferred from `x`.
    UwidenLow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uwiden_high x`. (Unary)
    /// Type inferred from `x`.
    UwidenHigh, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = iadd_pairwise x, y`. (Binary)
    /// Type inferred from `x`.
    IaddPairwise, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = x86_pmaddubsw x, y`. (Binary)
    X86Pmaddubsw, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = uextend x`. (Unary)
    Uextend, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = sextend x`. (Unary)
    Sextend, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fpromote x`. (Unary)
    Fpromote, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fdemote x`. (Unary)
    Fdemote, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fvdemote x`. (Unary)
    Fvdemote, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `x = fvpromote_low a`. (Unary)
    FvpromoteLow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fcvt_to_uint x`. (Unary)
    FcvtToUint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fcvt_to_sint x`. (Unary)
    FcvtToSint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fcvt_to_uint_sat x`. (Unary)
    FcvtToUintSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fcvt_to_sint_sat x`. (Unary)
    FcvtToSintSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = x86_cvtt2dq x`. (Unary)
    X86Cvtt2dq, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fcvt_from_uint x`. (Unary)
    FcvtFromUint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = fcvt_from_sint x`. (Unary)
    FcvtFromSint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `lo, hi = isplit x`. (Unary)
    /// Type inferred from `x`.
    Isplit, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = iconcat lo, hi`. (Binary)
    /// Type inferred from `lo`.
    Iconcat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = atomic_rmw MemFlags, AtomicRmwOp, p, x`. (AtomicRmw)
    AtomicRmw, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = atomic_cas MemFlags, p, e, x`. (AtomicCas)
    /// Type inferred from `x`.
    AtomicCas, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = atomic_load MemFlags, p`. (LoadNoOffset)
    AtomicLoad, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `atomic_store MemFlags, x, p`. (StoreNoOffset)
    /// Type inferred from `x`.
    AtomicStore, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `fence`. (NullAry)
    Fence, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `a = extract_vector x, y`. (BinaryImm8)
    /// Type inferred from `x`.
    ExtractVector, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `sequence_point`. (NullAry)
    SequencePoint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
    /// `patchable_call FN, args`. (Call)
    PatchableCall, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:636
}

impl Opcode {
    /// True for instructions that terminate the block
    pub fn is_terminator(self) -> bool {
        match self {
            Self::BrTable |
            Self::Brif |
            Self::Jump |
            Self::Return |
            Self::ReturnCall |
            Self::ReturnCallIndirect |
            Self::Trap |
            Self::TryCall |
            Self::TryCallIndirect => {
                true
            }
            _ => {
                false
            }
        }
    }

    /// True for all branch or jump instructions.
    pub fn is_branch(self) -> bool {
        match self {
            Self::BrTable |
            Self::Brif |
            Self::Jump |
            Self::TryCall |
            Self::TryCallIndirect => {
                true
            }
            _ => {
                false
            }
        }
    }

    /// Is this a call instruction?
    pub fn is_call(self) -> bool {
        match self {
            Self::Call |
            Self::CallIndirect |
            Self::PatchableCall |
            Self::ReturnCall |
            Self::ReturnCallIndirect |
            Self::StackSwitch |
            Self::TryCall |
            Self::TryCallIndirect => {
                true
            }
            _ => {
                false
            }
        }
    }

    /// Is this a return instruction?
    pub fn is_return(self) -> bool {
        match self {
            Self::Return |
            Self::ReturnCall |
            Self::ReturnCallIndirect => {
                true
            }
            _ => {
                false
            }
        }
    }

    /// Can this instruction read from memory?
    pub fn can_load(self) -> bool {
        match self {
            Self::AtomicCas |
            Self::AtomicLoad |
            Self::AtomicRmw |
            Self::Debugtrap |
            Self::DynamicStackLoad |
            Self::Load |
            Self::Sload16 |
            Self::Sload16x4 |
            Self::Sload32 |
            Self::Sload32x2 |
            Self::Sload8 |
            Self::Sload8x8 |
            Self::StackLoad |
            Self::StackSwitch |
            Self::Uload16 |
            Self::Uload16x4 |
            Self::Uload32 |
            Self::Uload32x2 |
            Self::Uload8 |
            Self::Uload8x8 => {
                true
            }
            _ => {
                false
            }
        }
    }

    /// Can this instruction write to memory?
    pub fn can_store(self) -> bool {
        match self {
            Self::AtomicCas |
            Self::AtomicRmw |
            Self::AtomicStore |
            Self::Debugtrap |
            Self::DynamicStackStore |
            Self::Istore16 |
            Self::Istore32 |
            Self::Istore8 |
            Self::StackStore |
            Self::StackSwitch |
            Self::Store => {
                true
            }
            _ => {
                false
            }
        }
    }

    /// Can this instruction cause a trap?
    pub fn can_trap(self) -> bool {
        match self {
            Self::FcvtToSint |
            Self::FcvtToUint |
            Self::Sdiv |
            Self::Srem |
            Self::Trap |
            Self::Trapnz |
            Self::Trapz |
            Self::UaddOverflowTrap |
            Self::Udiv |
            Self::Urem => {
                true
            }
            _ => {
                false
            }
        }
    }

    /// Does this instruction have other side effects besides can_* flags?
    pub fn other_side_effects(self) -> bool {
        match self {
            Self::AtomicCas |
            Self::AtomicLoad |
            Self::AtomicRmw |
            Self::AtomicStore |
            Self::Debugtrap |
            Self::Fence |
            Self::GetPinnedReg |
            Self::SequencePoint |
            Self::SetPinnedReg |
            Self::StackSwitch => {
                true
            }
            _ => {
                false
            }
        }
    }

    /// Despite having side effects, is this instruction okay to GVN?
    pub fn side_effects_idempotent(self) -> bool {
        match self {
            Self::FcvtToSint |
            Self::FcvtToUint |
            Self::Sdiv |
            Self::Srem |
            Self::Trapnz |
            Self::Trapz |
            Self::UaddOverflowTrap |
            Self::Udiv |
            Self::Urem => {
                true
            }
            _ => {
                false
            }
        }
    }

    /// All cranelift opcodes.
    pub fn all() -> &'static [Opcode] {
        return &[
            Opcode::Jump, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Brif, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::BrTable, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Debugtrap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Trap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Trapz, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Trapnz, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Return, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Call, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::CallIndirect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::ReturnCall, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::ReturnCallIndirect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::FuncAddr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::TryCall, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::TryCallIndirect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Splat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Swizzle, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::X86Pshufb, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Insertlane, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Extractlane, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Smin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Umin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Smax, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Umax, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::AvgRound, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UaddSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SaddSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UsubSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SsubSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Load, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Store, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Uload8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sload8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Istore8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Uload16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sload16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Istore16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Uload32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sload32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Istore32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::StackSwitch, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Uload8x8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sload8x8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Uload16x4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sload16x4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Uload32x2, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sload32x2, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::StackLoad, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::StackStore, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::StackAddr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::DynamicStackLoad, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::DynamicStackStore, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::DynamicStackAddr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::GlobalValue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SymbolValue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::TlsValue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::GetPinnedReg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SetPinnedReg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::GetFramePointer, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::GetStackPointer, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::GetReturnAddress, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::GetExceptionHandlerAddress, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Iconst, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::F16const, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::F32const, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::F64const, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::F128const, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Vconst, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Shuffle, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Nop, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Select, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SelectSpectreGuard, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Bitselect, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::X86Blendv, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::VanyTrue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::VallTrue, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::VhighBits, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Icmp, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::IcmpImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Iadd, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Isub, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Ineg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Iabs, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Imul, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Umulhi, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Smulhi, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SqmulRoundSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::X86Pmulhrsw, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Udiv, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sdiv, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Urem, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Srem, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::IaddImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::ImulImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UdivImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SdivImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UremImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SremImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::IrsubImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SaddOverflowCin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UaddOverflowCin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UaddOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SaddOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UsubOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SsubOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UmulOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SmulOverflow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UaddOverflowTrap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SsubOverflowBin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UsubOverflowBin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Band, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Bor, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Bxor, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Bnot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::BandNot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::BorNot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::BxorNot, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::BandImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::BorImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::BxorImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Rotl, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Rotr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::RotlImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::RotrImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Ishl, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Ushr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sshr, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::IshlImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UshrImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SshrImm, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Bitrev, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Clz, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Cls, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Ctz, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Bswap, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Popcnt, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fcmp, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fadd, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fsub, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fmul, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fdiv, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sqrt, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fma, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fneg, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fabs, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fcopysign, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fmin, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fmax, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Ceil, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Floor, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Trunc, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Nearest, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Bitcast, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::ScalarToVector, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Bmask, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Ireduce, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Snarrow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Unarrow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Uunarrow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SwidenLow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SwidenHigh, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UwidenLow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::UwidenHigh, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::IaddPairwise, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::X86Pmaddubsw, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Uextend, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Sextend, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fpromote, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fdemote, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fvdemote, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::FvpromoteLow, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::FcvtToUint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::FcvtToSint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::FcvtToUintSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::FcvtToSintSat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::X86Cvtt2dq, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::FcvtFromUint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::FcvtFromSint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Isplit, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Iconcat, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::AtomicRmw, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::AtomicCas, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::AtomicLoad, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::AtomicStore, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::Fence, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::ExtractVector, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::SequencePoint, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
            Opcode::PatchableCall, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:713
        ];
    }

}

const OPCODE_FORMAT: [InstructionFormat; 188] = [ // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:723
    InstructionFormat::Jump, // jump // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Brif, // brif // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BranchTable, // br_table // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::NullAry, // debugtrap // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Trap, // trap // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::CondTrap, // trapz // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::CondTrap, // trapnz // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::MultiAry, // return // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Call, // call // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::CallIndirect, // call_indirect // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Call, // return_call // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::CallIndirect, // return_call_indirect // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::FuncAddr, // func_addr // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::TryCall, // try_call // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::TryCallIndirect, // try_call_indirect // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // splat // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // swizzle // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // x86_pshufb // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::TernaryImm8, // insertlane // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm8, // extractlane // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // smin // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // umin // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // smax // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // umax // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // avg_round // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // uadd_sat // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // sadd_sat // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // usub_sat // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // ssub_sat // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // load // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Store, // store // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // uload8 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // sload8 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Store, // istore8 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // uload16 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // sload16 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Store, // istore16 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // uload32 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // sload32 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Store, // istore32 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // stack_switch // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // uload8x8 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // sload8x8 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // uload16x4 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // sload16x4 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // uload32x2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Load, // sload32x2 // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::StackLoad, // stack_load // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::StackStore, // stack_store // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::StackLoad, // stack_addr // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::DynamicStackLoad, // dynamic_stack_load // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::DynamicStackStore, // dynamic_stack_store // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::DynamicStackLoad, // dynamic_stack_addr // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::UnaryGlobalValue, // global_value // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::UnaryGlobalValue, // symbol_value // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::UnaryGlobalValue, // tls_value // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::NullAry, // get_pinned_reg // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // set_pinned_reg // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::NullAry, // get_frame_pointer // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::NullAry, // get_stack_pointer // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::NullAry, // get_return_address // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::ExceptionHandlerAddress, // get_exception_handler_address // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::UnaryImm, // iconst // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::UnaryIeee16, // f16const // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::UnaryIeee32, // f32const // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::UnaryIeee64, // f64const // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::UnaryConst, // f128const // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::UnaryConst, // vconst // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Shuffle, // shuffle // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::NullAry, // nop // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // select // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // select_spectre_guard // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // bitselect // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // x86_blendv // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // vany_true // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // vall_true // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // vhigh_bits // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::IntCompare, // icmp // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::IntCompareImm, // icmp_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // iadd // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // isub // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // ineg // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // iabs // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // imul // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // umulhi // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // smulhi // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // sqmul_round_sat // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // x86_pmulhrsw // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // udiv // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // sdiv // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // urem // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // srem // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // iadd_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // imul_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // udiv_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // sdiv_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // urem_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // srem_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // irsub_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // sadd_overflow_cin // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // uadd_overflow_cin // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // uadd_overflow // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // sadd_overflow // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // usub_overflow // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // ssub_overflow // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // umul_overflow // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // smul_overflow // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::IntAddTrap, // uadd_overflow_trap // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // ssub_overflow_bin // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // usub_overflow_bin // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // band // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // bor // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // bxor // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // bnot // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // band_not // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // bor_not // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // bxor_not // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // band_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // bor_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // bxor_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // rotl // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // rotr // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // rotl_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // rotr_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // ishl // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // ushr // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // sshr // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // ishl_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // ushr_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm64, // sshr_imm // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // bitrev // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // clz // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // cls // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // ctz // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // bswap // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // popcnt // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::FloatCompare, // fcmp // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // fadd // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // fsub // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // fmul // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // fdiv // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // sqrt // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Ternary, // fma // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fneg // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fabs // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // fcopysign // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // fmin // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // fmax // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // ceil // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // floor // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // trunc // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // nearest // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::LoadNoOffset, // bitcast // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // scalar_to_vector // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // bmask // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // ireduce // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // snarrow // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // unarrow // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // uunarrow // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // swiden_low // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // swiden_high // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // uwiden_low // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // uwiden_high // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // iadd_pairwise // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // x86_pmaddubsw // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // uextend // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // sextend // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fpromote // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fdemote // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fvdemote // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fvpromote_low // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fcvt_to_uint // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fcvt_to_sint // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fcvt_to_uint_sat // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fcvt_to_sint_sat // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // x86_cvtt2dq // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fcvt_from_uint // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // fcvt_from_sint // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Unary, // isplit // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Binary, // iconcat // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::AtomicRmw, // atomic_rmw // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::AtomicCas, // atomic_cas // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::LoadNoOffset, // atomic_load // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::StoreNoOffset, // atomic_store // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::NullAry, // fence // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::BinaryImm8, // extract_vector // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::NullAry, // sequence_point // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
    InstructionFormat::Call, // patchable_call // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:730
]; // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:738

fn opcode_name(opc: Opcode) -> &'static str {
    match opc {
        Opcode::AtomicCas => {
            "atomic_cas"
        }
        Opcode::AtomicLoad => {
            "atomic_load"
        }
        Opcode::AtomicRmw => {
            "atomic_rmw"
        }
        Opcode::AtomicStore => {
            "atomic_store"
        }
        Opcode::AvgRound => {
            "avg_round"
        }
        Opcode::Band => {
            "band"
        }
        Opcode::BandImm => {
            "band_imm"
        }
        Opcode::BandNot => {
            "band_not"
        }
        Opcode::Bitcast => {
            "bitcast"
        }
        Opcode::Bitrev => {
            "bitrev"
        }
        Opcode::Bitselect => {
            "bitselect"
        }
        Opcode::Bmask => {
            "bmask"
        }
        Opcode::Bnot => {
            "bnot"
        }
        Opcode::Bor => {
            "bor"
        }
        Opcode::BorImm => {
            "bor_imm"
        }
        Opcode::BorNot => {
            "bor_not"
        }
        Opcode::BrTable => {
            "br_table"
        }
        Opcode::Brif => {
            "brif"
        }
        Opcode::Bswap => {
            "bswap"
        }
        Opcode::Bxor => {
            "bxor"
        }
        Opcode::BxorImm => {
            "bxor_imm"
        }
        Opcode::BxorNot => {
            "bxor_not"
        }
        Opcode::Call => {
            "call"
        }
        Opcode::CallIndirect => {
            "call_indirect"
        }
        Opcode::Ceil => {
            "ceil"
        }
        Opcode::Cls => {
            "cls"
        }
        Opcode::Clz => {
            "clz"
        }
        Opcode::Ctz => {
            "ctz"
        }
        Opcode::Debugtrap => {
            "debugtrap"
        }
        Opcode::DynamicStackAddr => {
            "dynamic_stack_addr"
        }
        Opcode::DynamicStackLoad => {
            "dynamic_stack_load"
        }
        Opcode::DynamicStackStore => {
            "dynamic_stack_store"
        }
        Opcode::ExtractVector => {
            "extract_vector"
        }
        Opcode::Extractlane => {
            "extractlane"
        }
        Opcode::F128const => {
            "f128const"
        }
        Opcode::F16const => {
            "f16const"
        }
        Opcode::F32const => {
            "f32const"
        }
        Opcode::F64const => {
            "f64const"
        }
        Opcode::Fabs => {
            "fabs"
        }
        Opcode::Fadd => {
            "fadd"
        }
        Opcode::Fcmp => {
            "fcmp"
        }
        Opcode::Fcopysign => {
            "fcopysign"
        }
        Opcode::FcvtFromSint => {
            "fcvt_from_sint"
        }
        Opcode::FcvtFromUint => {
            "fcvt_from_uint"
        }
        Opcode::FcvtToSint => {
            "fcvt_to_sint"
        }
        Opcode::FcvtToSintSat => {
            "fcvt_to_sint_sat"
        }
        Opcode::FcvtToUint => {
            "fcvt_to_uint"
        }
        Opcode::FcvtToUintSat => {
            "fcvt_to_uint_sat"
        }
        Opcode::Fdemote => {
            "fdemote"
        }
        Opcode::Fdiv => {
            "fdiv"
        }
        Opcode::Fence => {
            "fence"
        }
        Opcode::Floor => {
            "floor"
        }
        Opcode::Fma => {
            "fma"
        }
        Opcode::Fmax => {
            "fmax"
        }
        Opcode::Fmin => {
            "fmin"
        }
        Opcode::Fmul => {
            "fmul"
        }
        Opcode::Fneg => {
            "fneg"
        }
        Opcode::Fpromote => {
            "fpromote"
        }
        Opcode::Fsub => {
            "fsub"
        }
        Opcode::FuncAddr => {
            "func_addr"
        }
        Opcode::Fvdemote => {
            "fvdemote"
        }
        Opcode::FvpromoteLow => {
            "fvpromote_low"
        }
        Opcode::GetExceptionHandlerAddress => {
            "get_exception_handler_address"
        }
        Opcode::GetFramePointer => {
            "get_frame_pointer"
        }
        Opcode::GetPinnedReg => {
            "get_pinned_reg"
        }
        Opcode::GetReturnAddress => {
            "get_return_address"
        }
        Opcode::GetStackPointer => {
            "get_stack_pointer"
        }
        Opcode::GlobalValue => {
            "global_value"
        }
        Opcode::Iabs => {
            "iabs"
        }
        Opcode::Iadd => {
            "iadd"
        }
        Opcode::IaddImm => {
            "iadd_imm"
        }
        Opcode::IaddPairwise => {
            "iadd_pairwise"
        }
        Opcode::Icmp => {
            "icmp"
        }
        Opcode::IcmpImm => {
            "icmp_imm"
        }
        Opcode::Iconcat => {
            "iconcat"
        }
        Opcode::Iconst => {
            "iconst"
        }
        Opcode::Imul => {
            "imul"
        }
        Opcode::ImulImm => {
            "imul_imm"
        }
        Opcode::Ineg => {
            "ineg"
        }
        Opcode::Insertlane => {
            "insertlane"
        }
        Opcode::Ireduce => {
            "ireduce"
        }
        Opcode::IrsubImm => {
            "irsub_imm"
        }
        Opcode::Ishl => {
            "ishl"
        }
        Opcode::IshlImm => {
            "ishl_imm"
        }
        Opcode::Isplit => {
            "isplit"
        }
        Opcode::Istore16 => {
            "istore16"
        }
        Opcode::Istore32 => {
            "istore32"
        }
        Opcode::Istore8 => {
            "istore8"
        }
        Opcode::Isub => {
            "isub"
        }
        Opcode::Jump => {
            "jump"
        }
        Opcode::Load => {
            "load"
        }
        Opcode::Nearest => {
            "nearest"
        }
        Opcode::Nop => {
            "nop"
        }
        Opcode::PatchableCall => {
            "patchable_call"
        }
        Opcode::Popcnt => {
            "popcnt"
        }
        Opcode::Return => {
            "return"
        }
        Opcode::ReturnCall => {
            "return_call"
        }
        Opcode::ReturnCallIndirect => {
            "return_call_indirect"
        }
        Opcode::Rotl => {
            "rotl"
        }
        Opcode::RotlImm => {
            "rotl_imm"
        }
        Opcode::Rotr => {
            "rotr"
        }
        Opcode::RotrImm => {
            "rotr_imm"
        }
        Opcode::SaddOverflow => {
            "sadd_overflow"
        }
        Opcode::SaddOverflowCin => {
            "sadd_overflow_cin"
        }
        Opcode::SaddSat => {
            "sadd_sat"
        }
        Opcode::ScalarToVector => {
            "scalar_to_vector"
        }
        Opcode::Sdiv => {
            "sdiv"
        }
        Opcode::SdivImm => {
            "sdiv_imm"
        }
        Opcode::Select => {
            "select"
        }
        Opcode::SelectSpectreGuard => {
            "select_spectre_guard"
        }
        Opcode::SequencePoint => {
            "sequence_point"
        }
        Opcode::SetPinnedReg => {
            "set_pinned_reg"
        }
        Opcode::Sextend => {
            "sextend"
        }
        Opcode::Shuffle => {
            "shuffle"
        }
        Opcode::Sload16 => {
            "sload16"
        }
        Opcode::Sload16x4 => {
            "sload16x4"
        }
        Opcode::Sload32 => {
            "sload32"
        }
        Opcode::Sload32x2 => {
            "sload32x2"
        }
        Opcode::Sload8 => {
            "sload8"
        }
        Opcode::Sload8x8 => {
            "sload8x8"
        }
        Opcode::Smax => {
            "smax"
        }
        Opcode::Smin => {
            "smin"
        }
        Opcode::SmulOverflow => {
            "smul_overflow"
        }
        Opcode::Smulhi => {
            "smulhi"
        }
        Opcode::Snarrow => {
            "snarrow"
        }
        Opcode::Splat => {
            "splat"
        }
        Opcode::SqmulRoundSat => {
            "sqmul_round_sat"
        }
        Opcode::Sqrt => {
            "sqrt"
        }
        Opcode::Srem => {
            "srem"
        }
        Opcode::SremImm => {
            "srem_imm"
        }
        Opcode::Sshr => {
            "sshr"
        }
        Opcode::SshrImm => {
            "sshr_imm"
        }
        Opcode::SsubOverflow => {
            "ssub_overflow"
        }
        Opcode::SsubOverflowBin => {
            "ssub_overflow_bin"
        }
        Opcode::SsubSat => {
            "ssub_sat"
        }
        Opcode::StackAddr => {
            "stack_addr"
        }
        Opcode::StackLoad => {
            "stack_load"
        }
        Opcode::StackStore => {
            "stack_store"
        }
        Opcode::StackSwitch => {
            "stack_switch"
        }
        Opcode::Store => {
            "store"
        }
        Opcode::SwidenHigh => {
            "swiden_high"
        }
        Opcode::SwidenLow => {
            "swiden_low"
        }
        Opcode::Swizzle => {
            "swizzle"
        }
        Opcode::SymbolValue => {
            "symbol_value"
        }
        Opcode::TlsValue => {
            "tls_value"
        }
        Opcode::Trap => {
            "trap"
        }
        Opcode::Trapnz => {
            "trapnz"
        }
        Opcode::Trapz => {
            "trapz"
        }
        Opcode::Trunc => {
            "trunc"
        }
        Opcode::TryCall => {
            "try_call"
        }
        Opcode::TryCallIndirect => {
            "try_call_indirect"
        }
        Opcode::UaddOverflow => {
            "uadd_overflow"
        }
        Opcode::UaddOverflowCin => {
            "uadd_overflow_cin"
        }
        Opcode::UaddOverflowTrap => {
            "uadd_overflow_trap"
        }
        Opcode::UaddSat => {
            "uadd_sat"
        }
        Opcode::Udiv => {
            "udiv"
        }
        Opcode::UdivImm => {
            "udiv_imm"
        }
        Opcode::Uextend => {
            "uextend"
        }
        Opcode::Uload16 => {
            "uload16"
        }
        Opcode::Uload16x4 => {
            "uload16x4"
        }
        Opcode::Uload32 => {
            "uload32"
        }
        Opcode::Uload32x2 => {
            "uload32x2"
        }
        Opcode::Uload8 => {
            "uload8"
        }
        Opcode::Uload8x8 => {
            "uload8x8"
        }
        Opcode::Umax => {
            "umax"
        }
        Opcode::Umin => {
            "umin"
        }
        Opcode::UmulOverflow => {
            "umul_overflow"
        }
        Opcode::Umulhi => {
            "umulhi"
        }
        Opcode::Unarrow => {
            "unarrow"
        }
        Opcode::Urem => {
            "urem"
        }
        Opcode::UremImm => {
            "urem_imm"
        }
        Opcode::Ushr => {
            "ushr"
        }
        Opcode::UshrImm => {
            "ushr_imm"
        }
        Opcode::UsubOverflow => {
            "usub_overflow"
        }
        Opcode::UsubOverflowBin => {
            "usub_overflow_bin"
        }
        Opcode::UsubSat => {
            "usub_sat"
        }
        Opcode::Uunarrow => {
            "uunarrow"
        }
        Opcode::UwidenHigh => {
            "uwiden_high"
        }
        Opcode::UwidenLow => {
            "uwiden_low"
        }
        Opcode::VallTrue => {
            "vall_true"
        }
        Opcode::VanyTrue => {
            "vany_true"
        }
        Opcode::Vconst => {
            "vconst"
        }
        Opcode::VhighBits => {
            "vhigh_bits"
        }
        Opcode::X86Blendv => {
            "x86_blendv"
        }
        Opcode::X86Cvtt2dq => {
            "x86_cvtt2dq"
        }
        Opcode::X86Pmaddubsw => {
            "x86_pmaddubsw"
        }
        Opcode::X86Pmulhrsw => {
            "x86_pmulhrsw"
        }
        Opcode::X86Pshufb => {
            "x86_pshufb"
        }
    }
}

const OPCODE_HASH_TABLE: [Option<Opcode>; 256] = [ // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:759
    Some(Opcode::Imul), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::TlsValue), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Brif), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Nearest), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::FcvtToSintSat), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fsub), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Trunc), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Urem), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Iconst), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::ReturnCall), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Umin), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Store), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::GetFramePointer), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::UshrImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Isub), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::FcvtFromSint), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Trap), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Sdiv), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Srem), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SshrImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Uunarrow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::UaddOverflowCin), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Bxor), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::X86Pmaddubsw), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Umax), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SremImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Insertlane), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::BxorNot), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Swizzle), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Load), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fadd), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Jump), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::BxorImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Shuffle), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fneg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Umulhi), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Ushr), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::UaddOverflowTrap), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::FcvtFromUint), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::VallTrue), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Band), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::SsubOverflow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Uload16x4), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Ishl), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fmax), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Vconst), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Call), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::ExtractVector), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Sqrt), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Ceil), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Ineg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::FuncAddr), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SaddSat), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Popcnt), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Fabs), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fmin), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SsubOverflowBin), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::GlobalValue), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Bnot), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Sextend), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Isplit), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::FcvtToUint), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::RotlImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fcmp), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SwidenHigh), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fmul), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::FcvtToSint), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::UsubOverflow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Uload8x8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Fdiv), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::UremImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::AtomicLoad), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Trapnz), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Uload16), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::IaddImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Uload32), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Bitrev), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Smulhi), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::TryCall), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::PatchableCall), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::BorNot), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Sload8x8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::X86Blendv), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SetPinnedReg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::ImulImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Ireduce), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::RotrImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::DynamicStackStore), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::StackStore), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::UwidenLow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Select), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::BorImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Istore32), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::FvpromoteLow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Istore16), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Fdemote), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::IcmpImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fvdemote), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Sload16), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fcopysign), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::SdivImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Unarrow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::AvgRound), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Sload32), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::X86Pshufb), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Extractlane), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::StackAddr), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SaddOverflowCin), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::UaddOverflow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::BandImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Return), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Uload32x2), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::VanyTrue), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::UsubSat), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::GetExceptionHandlerAddress), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::DynamicStackLoad), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Iconcat), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SmulOverflow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Fence), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Fma), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Bitselect), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Istore8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::BrTable), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::F64const), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::StackSwitch), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::StackLoad), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::IrsubImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Nop), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SqmulRoundSat), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::X86Pmulhrsw), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Debugtrap), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Sload16x4), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::UmulOverflow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::IshlImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SaddOverflow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Ctz), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Bor), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::TryCallIndirect), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::BandNot), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Clz), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::UwidenHigh), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Uextend), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Floor), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::UaddSat), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Sload32x2), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SequencePoint), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::SelectSpectreGuard), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Cls), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Fpromote), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Bitcast), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::SymbolValue), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::DynamicStackAddr), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Bmask), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::GetPinnedReg), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SsubSat), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::AtomicRmw), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::ScalarToVector), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Uload8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::FcvtToUintSat), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Smin), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Trapz), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Iabs), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::F16const), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Udiv), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::AtomicCas), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::GetReturnAddress), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::UsubOverflowBin), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::SwidenLow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::ReturnCallIndirect), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Rotl), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::IaddPairwise), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Smax), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::F128const), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::F32const), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::UdivImm), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Splat), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Rotr), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Snarrow), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::CallIndirect), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Sload8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::X86Cvtt2dq), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::VhighBits), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Iadd), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Icmp), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::GetStackPointer), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::Bswap), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
    Some(Opcode::Sshr), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    Some(Opcode::AtomicStore), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:767
    None, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:768
]; // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:772


// Table of opcode constraints.
const OPCODE_CONSTRAINTS: [OpcodeConstraints; 188] = [ // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:893
    // Jump: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Brif: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x38, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // BrTable: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Concrete(ir::types::I32)']
    OpcodeConstraints {
        flags: 0x20, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 3, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Debugtrap: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Trap: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Trapz: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x38, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Trapnz: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x38, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Return: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Call: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // CallIndirect: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x38, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // ReturnCall: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // ReturnCallIndirect: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x38, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // FuncAddr: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // TryCall: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // TryCallIndirect: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x38, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Splat: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'LaneOf']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 2, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Swizzle: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Concrete(ir::types::I8X16)', 'Concrete(ir::types::I8X16)', 'Concrete(ir::types::I8X16)']
    OpcodeConstraints {
        flags: 0x41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 6, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // X86Pshufb: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Concrete(ir::types::I8X16)', 'Concrete(ir::types::I8X16)', 'Concrete(ir::types::I8X16)']
    OpcodeConstraints {
        flags: 0x41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 6, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Insertlane: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'LaneOf']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 2, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 9, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Extractlane: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['LaneOf', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 2, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Smin: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 3, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Umin: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 3, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Smax: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 3, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Umax: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 3, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // AvgRound: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UaddSat: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SaddSat: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UsubSat: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SsubSat: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Load: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Store: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x58, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Uload8: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1}, ints={16, 32, 64})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 6, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sload8: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1}, ints={16, 32, 64})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 6, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Istore8: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1}, ints={16, 32, 64})
    OpcodeConstraints {
        flags: 0x58, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 6, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Uload16: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sload16: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Istore16: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x58, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Uload32: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I64)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sload32: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I64)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Istore32: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['Concrete(ir::types::I64)', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1}, ints={64})
    OpcodeConstraints {
        flags: 0x58, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 7, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // StackSwitch: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x69, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 18, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Uload8x8: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I16X8)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 22, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sload8x8: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I16X8)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 22, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Uload16x4: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I32X4)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 24, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sload16x4: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I32X4)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 24, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Uload32x2: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I64X2)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 26, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sload32x2: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I64X2)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 26, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // StackLoad: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // StackStore: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x38, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // StackAddr: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // DynamicStackLoad: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // DynamicStackStore: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x38, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // DynamicStackAddr: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // GlobalValue: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SymbolValue: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // TlsValue: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // GetPinnedReg: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SetPinnedReg: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x38, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // GetFramePointer: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // GetStackPointer: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // GetReturnAddress: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // GetExceptionHandlerAddress: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Iconst: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // F16const: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Concrete(ir::types::F16)']
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 28, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // F32const: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Concrete(ir::types::F32)']
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // F64const: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Concrete(ir::types::F64)']
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 30, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // F128const: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Concrete(ir::types::F128)']
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 31, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Vconst: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=['Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x01, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 9, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Shuffle: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Concrete(ir::types::I8X16)', 'Concrete(ir::types::I8X16)', 'Concrete(ir::types::I8X16)']
    OpcodeConstraints {
        flags: 0x41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 6, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Nop: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Select: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Free(0)', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x69, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SelectSpectreGuard: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Free(0)', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x69, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Bitselect: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x69, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 18, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // X86Blendv: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x69, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 18, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // VanyTrue: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I8)', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 9, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 36, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // VallTrue: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I8)', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 9, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 36, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // VhighBits: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(9)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 37, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Icmp: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['AsTruthy', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x59, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // IcmpImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['Concrete(ir::types::I8)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 36, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Iadd: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Isub: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Ineg: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Iabs: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Imul: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Umulhi: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Smulhi: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SqmulRoundSat: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={4, 8}, ints={16, 32})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // X86Pmulhrsw: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={4, 8}, ints={16, 32})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Udiv: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sdiv: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Urem: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Srem: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // IaddImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // ImulImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UdivImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SdivImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UremImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SremImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // IrsubImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SaddOverflowCin: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same', 'Concrete(ir::types::I8)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x6a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UaddOverflowCin: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same', 'Concrete(ir::types::I8)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x6a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UaddOverflow: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x4a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SaddOverflow: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x4a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UsubOverflow: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x4a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SsubOverflow: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x4a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UmulOverflow: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64})
    OpcodeConstraints {
        flags: 0x4a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SmulOverflow: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64})
    OpcodeConstraints {
        flags: 0x4a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UaddOverflowTrap: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={32, 64})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 1, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SsubOverflowBin: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same', 'Concrete(ir::types::I8)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x6a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UsubOverflowBin: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Concrete(ir::types::I8)', 'Same', 'Same', 'Concrete(ir::types::I8)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x6a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Band: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Bor: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Bxor: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Bnot: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // BandNot: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // BorNot: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // BxorNot: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 10, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // BandImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // BorImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // BxorImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Rotl: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Free(0)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 46, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Rotr: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Free(0)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 46, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // RotlImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // RotrImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Ishl: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Free(0)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 46, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Ushr: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Free(0)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 46, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sshr: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Free(0)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 46, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // IshlImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UshrImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SshrImm: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Bitrev: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Clz: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Cls: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Ctz: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Bswap: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 13, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Popcnt: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 11, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fcmp: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['AsTruthy', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x59, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fadd: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fsub: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fmul: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fdiv: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sqrt: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fma: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x69, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 18, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fneg: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fabs: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fcopysign: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fmin: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fmax: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Ceil: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Floor: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Trunc: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Nearest: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Same']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x29, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 14, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Bitcast: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(5)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 5, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // ScalarToVector: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'LaneOf']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 9, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 4, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Bmask: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(0)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 32, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Ireduce: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Wider']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 51, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Snarrow: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['SplitLanes', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8}, ints={16, 32, 64})
    OpcodeConstraints {
        flags: 0x59, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 15, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 53, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Unarrow: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['SplitLanes', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8}, ints={16, 32, 64})
    OpcodeConstraints {
        flags: 0x59, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 15, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 53, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Uunarrow: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['SplitLanes', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8}, ints={16, 32, 64})
    OpcodeConstraints {
        flags: 0x59, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 15, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 53, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SwidenLow: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['MergeLanes', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16}, ints={8, 16, 32})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 56, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SwidenHigh: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['MergeLanes', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16}, ints={8, 16, 32})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 56, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UwidenLow: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['MergeLanes', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16}, ints={8, 16, 32})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 56, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // UwidenHigh: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['MergeLanes', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16}, ints={8, 16, 32})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 56, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // IaddPairwise: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={2, 4, 8, 16}, ints={8, 16, 32})
    OpcodeConstraints {
        flags: 0x49, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 16, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // X86Pmaddubsw: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Concrete(ir::types::I16X8)', 'Concrete(ir::types::I8X16)', 'Concrete(ir::types::I8X16)']
    OpcodeConstraints {
        flags: 0x41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 58, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Uextend: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Narrower']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 61, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Sextend: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Narrower']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 61, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fpromote: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Narrower']
    // Polymorphic over TypeSet(lanes={1}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 17, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 61, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fdemote: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Wider']
    // Polymorphic over TypeSet(lanes={1}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 17, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 51, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fvdemote: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Concrete(ir::types::F32X4)', 'Concrete(ir::types::F64X2)']
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 63, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // FvpromoteLow: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Concrete(ir::types::F64X2)', 'Concrete(ir::types::F32X4)']
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 64, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // FcvtToUint: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(17)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 66, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // FcvtToSint: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(17)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 66, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // FcvtToUintSat: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(14)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 3, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 68, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // FcvtToSintSat: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(14)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 3, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 68, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // X86Cvtt2dq: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(14)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 3, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 68, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // FcvtFromUint: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(3)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 18, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 70, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // FcvtFromSint: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(3)']
    // Polymorphic over TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 18, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 70, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Isplit: fixed_results=2, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['HalfWidth', 'HalfWidth', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x3a, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 13, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 72, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Iconcat: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['DoubleWidth', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64})
    OpcodeConstraints {
        flags: 0x59, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 8, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 75, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // AtomicRmw: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=2
    // Constraints=['Same', 'Free(1)', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x41, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 77, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // AtomicCas: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=false, fixed_values=3
    // Constraints=['Same', 'Free(1)', 'Same', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x69, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 77, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // AtomicLoad: fixed_results=1, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=1
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x21, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // AtomicStore: fixed_results=0, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=2
    // Constraints=['Same', 'Free(1)']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x58, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 12, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // Fence: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // ExtractVector: fixed_results=1, use_typevar_operand=true, requires_typevar_operand=true, fixed_values=1
    // Constraints=['DynamicToVector', 'Same']
    // Polymorphic over TypeSet(lanes={1}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
    OpcodeConstraints {
        flags: 0x39, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 19, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 81, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // SequencePoint: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
    // PatchableCall: fixed_results=0, use_typevar_operand=false, requires_typevar_operand=false, fixed_values=0
    // Constraints=[]
    OpcodeConstraints {
        flags: 0x00, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:964
        typeset_offset: 255, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:965
        constraint_offset: 0, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:966
    }
    ,
]; // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:971

// Table of value type sets.
const TYPE_SETS: [ir::instructions::ValueTypeSet; 20] = [ // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:854
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1}, ints={8, 16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(248), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1}, ints={32, 64})
        lanes: ScalarBitSet::<u16>(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(96), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(510), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(510), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(248), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(240), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(511), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(248), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(510), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(248), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(511), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(510), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(248), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(240), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1}, ints={16, 32, 64})
        lanes: ScalarBitSet::<u16>(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(112), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1}, ints={64})
        lanes: ScalarBitSet::<u16>(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(64), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1}, ints={8, 16, 32, 64})
        lanes: ScalarBitSet::<u16>(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(120), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(510), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(248), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(240), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(511), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(248), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(240), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, ints={8, 16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(511), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(510), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(248), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={4, 8}, ints={16, 32})
        lanes: ScalarBitSet::<u16>(12), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(48), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1}, ints={16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(240), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(511), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(510), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(240), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={2, 4, 8}, ints={16, 32, 64})
        lanes: ScalarBitSet::<u16>(14), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(14), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(112), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={2, 4, 8, 16}, ints={8, 16, 32})
        lanes: ScalarBitSet::<u16>(30), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(30), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(56), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1}, floats={16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(240), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1, 2, 4, 8, 16, 32, 64, 128, 256}, floats={16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(511), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(240), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
    ir::instructions::ValueTypeSet {
        // TypeSet(lanes={1}, ints={8, 16, 32, 64, 128}, floats={16, 32, 64, 128})
        lanes: ScalarBitSet::<u16>(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        dynamic_lanes: ScalarBitSet::<u16>(510), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        ints: ScalarBitSet::<u8>(248), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
        floats: ScalarBitSet::<u8>(240), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:822
    }
    ,
]; // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:871

// Table of operand constraint sequences.
const OPERAND_CONSTRAINTS: [OperandConstraint; 83] = [ // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:978
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I32), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::LaneOf, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I8X16), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I8X16), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I8X16), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::LaneOf, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I64), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I64), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I16X8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I32X4), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I64X2), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::F16), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::F32), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::F64), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::F128), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(9), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::AsTruthy, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(0), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(5), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Wider, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::SplitLanes, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::MergeLanes, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I16X8), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I8X16), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::I8X16), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Narrower, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::F32X4), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::F64X2), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Concrete(ir::types::F32X4), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(17), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(14), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(3), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::HalfWidth, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::HalfWidth, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::DoubleWidth, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Free(1), // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::DynamicToVector, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
    OperandConstraint::Same, // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:985
]; // /home/clawdio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cranelift-codegen-meta-0.127.4/src/gen_inst.rs:988
