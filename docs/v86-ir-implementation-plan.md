# v86 完整 IR JIT：Implementation Plan

## 0. 工程目标与基线

**目标仓库：** `speedyHKjournalist/v86`。

**核对基线：** 2026-09-12 本次读取的默认 `master` 分支关键源文件。本文未固定远端 commit SHA；开始开发前必须记录实际 SHA，并以该 SHA 重新核对接口。本次交付是实施设计与任务计划，未修改仓库、运行编译或执行 Windows XP 镜像。

**目标：** 建设覆盖当前 v86 CPU 已有能力的完整 IR 编译管线，最终同时替代 Tier 1 与 Tier 2 的旧式指令发射路径。Tier 0 解释器继续作为冷执行、合法运行时退出及差分参考，不要求删除解释器。

最终结构：

```text
x86 字节与编译上下文快照
  → 共享解码器 / DecodedInstruction
  → HIR：SSA 值 + x86 语义 + 副作用顺序 + 状态恢复信息
  → Tier 1 轻量优化 / Tier 2 区域级优化
  → MIR：显式 RAM 快慢路径、helper ABI、退出与状态物化
  → SSA 消除、CFG 结构化、local 分配、Wasm 栈调度
  → Wasm 编码与验证
  → 现有执行框架下的模块发布、查找、链接、失效、回收
```

这是完整重构项目，不以一个热循环原型作为最终交付；分阶段合入仅用于控制正确性和回归风险。

### 0.1 “完整”的定义

| 维度 | 完成条件 |
|---|---|
| 编译入口 | 正常生产构建的 Tier 1、Tier 2 都经 IR 管线 |
| 指令覆盖 | 对固定基线支持的编码、前缀、操作数形式和执行模式，都有明确的 IR lowering 或有契约的语义 helper |
| 高性能覆盖 | 普通整数、分支、地址计算、常规访存及已有可直接映射的 SIMD 不能长期退化成通用逐指令 helper |
| 系统语义 | 异常、部分完成、分段、分页、控制状态变化、I/O、预算退出、代码失效均进入设计 |
| 编译工程 | verifier、dump、差分测试、回归测试、统计、预算、取消、回滚路径齐全 |
| 旧路径 | 最终生产路径不再调用旧 `jit_instruction()` 或旧指令专用 Wasm 发射器 |

“有契约的 helper”仍属于完整 IR：它必须是显式 IR 节点，拥有输入输出、状态读写、访存、异常和控制流语义；不能用 `InterpretOne(opcode)` 或“跳回旧 JIT 生成这条指令”掩盖未完成的 lowering。

当前 v86 README 列出的已有缺失能力，不自动纳入本次补全 ISA 的范围。工程目标是覆盖和保持固定基线已有能力，不把 x86-64、SMP、AVX 或完整 Intel CPU 重实现混进本项目。[S1]

### 0.2 明确不改的边界

保持 CPU 状态对外布局、现有设备接口、镜像格式、快照格式及 graphics proxy/PCI 协议兼容。新增 JIT 缓存和编译元数据只属于运行时，不序列化进客户机状态。恢复快照时重建这些元数据。

默认采用严格语义：不以 f64 普遍替代 x87，不为了跑分修改客户机计时，不通过改变 CPUID 或测试线程条件制造收益。既有 CPU 模型中的限制单独登记；对其修复必须有独立测试和说明，不能混在优化 pass 中静默改变。

---

## 1. 当前代码接入地图

以下是核对过的接入点；右列是拟实施改动，不表示这些新模块已经存在。

| 现有路径 | 已核对职责 | 拟实施改动 |
|---|---|---|
| `gen/x86_table.js` | 指令编码、前缀、形式及测试属性 [S2] | 扩展或关联语义分类与覆盖清单，作为编码目录的唯一来源 |
| `gen/generate_jit.js` | 生成分派，选择自定义 JIT 发射或 CPU helper [S3] | 抽取共享解码生成逻辑，新增 IR decoder/dispatch 生成目标 |
| `src/rust/cpu_context.rs`、`modrm.rs` | 编译期取字节、模式和寻址解析 [S4] | 拆出无运行时副作用的解码接口，明确逻辑、线性、物理地址 |
| `src/rust/analysis.rs` | 控制流指令分析 [S5] | 逐步改为消费共享 decoded 结果，避免再解码语义分歧 |
| `src/rust/jit.rs` | 区域发现、层级、生成、发布和失效 [S6] | 形成后端接口；把 IR 编译过程从缓存生命周期中分离 |
| `src/rust/jit_instructions.rs`、`codegen.rs` | 旧指令与公共 Wasm 发射逻辑 [S7] | 作为迁移参考；抽出真正与旧 JitContext 无关的可复用部件 |
| `src/rust/simd_codegen.rs` | SIMD 发射与缓存 [S8] | 迁移语义和已验证快路径，不原封不动包成黑盒节点 |
| `src/rust/control_flow.rs` | CFG、loopify、blockify [S9] | 适配显式 IR BlockId，支持边复制和 dispatcher 回退 |
| `src/rust/wasmgen/wasm_builder.rs` | Wasm 编码、locals、类型和延迟写回 [S10] | 扩展索引与签名，成为通用编码层，避免重复优化状态 |
| `src/rust/cpu/cpu.rs` | 执行、helper、内存转换、链接等 [S11] | 增加适配后的 helper 契约、退出和升档接口 |
| `src/cpu.js` | Wasm 实例化和 table 安装 [S12] | 为编译结果加入 job/generation 校验及一致的失败处理 |
| `Makefile`、`tests/Readme.md` | 现有构建、CPU/JIT/分页/系统测试 [S13][S14] | 增加 IR 专属目标和 backend 参数化，不覆盖旧基准 |

重要：`docs/how-it-works.md` 的部分说明可能落后于 fork 实现。设计以实际代码为准，不依据旧文档重新“添加”已经存在的链接、Tier 或 SIMD 缓存。

### 1.1 必须前置：WasmBuilder 容量与编码审计

当前 builder 的 `local_count` 和 local 索引类型使用 `u8`；部分 local 指令直接写入单字节索引，局部变量分组也存在 `<128` 的假设。[S10]

SSA 区域扩大后不能依赖这些旧容量假设。单独提交：

- local、type、function、table 和相关计数按对应 Wasm 索引类型存储；需要的位置采用完整的 unsigned LEB128 编码。
- 类型表改为按结构签名驻留，或提供等价的可扩展签名机制；覆盖实际用到的 i32/i64/f32/f64/v128 参数与返回值。v128 helper 不跨 JS ABI，必要时用明确的内存 scratch ABI。
- 分组数量、组内数量、section/body 长度、分支深度和 import 数量一起审计，不能只把 Rust 字段改成 u32。
- 增加 127/128、255/256、1023/1024 附近的编码和执行测试；这些是边界测试，不是建议热区域实际使用如此多 locals。
- 验证局部变量生命期复用、类型一致性及错误恢复；拒绝因 release 下整数截断生成错误模块。

---

## 2. 目录与组件设计

以下均为拟新增目录；现有运行时无需一次移动到新目录。

```text
src/rust/ir/
  mod.rs
  ids.rs                  # ValueId / BlockId / InstId / StateId
  types.rs                # 值类型、地址类别、位宽
  opcode.rs               # HIR/MIR 操作的定义或分文件定义
  hir.rs                  # 区域、block 参数、节点、终结指令
  mir.rs                  # Wasm 可合法化操作、快慢路径、退出
  builder.rs              # sealed-block SSA 构造
  effects.rs              # effect token、alias class、状态读写集合
  state.rs                # GuestState、FlagState、StateMap
  helper.rs               # helper 元数据与 ABI 适配
  verify.rs               # 类型、支配、effect、恢复、guard 校验
  dump.rs                 # 文本 dump、CFG、变换统计
  frontend/
    decode.rs             # DecodedInstruction；只读代码快照
    lift.rs               # 编码/语义分类到 HIR
    integer.rs
    flags.rs
    control.rs
    memory.rs
    string.rs
    system.rs
    simd.rs
    x87.rs
  analysis/
    cfg.rs
    dominance.rs
    liveness.rs
    alias.rs
    loops.rs
    range.rs
  passes/
    canonicalize.rs
    const_fold.rs
    copy_prop.rs
    dce.rs
    flags.rs
    gvn.rs
    cfg_simplify.rs
    state_elision.rs
    address_reuse.rs
    memory.rs
    licm.rs
    loop_opts.rs
    simd.rs
  lowering/
    hir_to_mir.rs
    memory.rs
    helper_calls.rs
    state_maps.rs
    fp.rs
  backend/
    phi.rs
    structure.rs
    locals.rs
    schedule.rs
    wasm.rs
  runtime/
    compile.rs
    region.rs
    dependencies.rs
    exits.rs
    counters.rs

gen/
  generate_ir_decoder.js
  ir_semantics.js          # 若不直接扩展 x86_table，按稳定 encoding_id 关联

tests/ir/
  decode/
  semantics/
  optimization/
  exceptions/
  memory/
  invalidation/
  wasm/
  differential/
  workloads/

docs/
  ir-design.md
  ir-helper-contracts.md
  ir-coverage.md
  ir-validation.md
  ir-performance.md
```

使用项目现有 Rust/JS 工具链。采用 arena + 连续向量 + 稳定整数 ID，避免每个节点单独 Box、String 和 HashMap。冷元数据与热节点分开保存，区域结束后批量释放。

不把 LLVM、MLIR、原生机器码 JIT 或一整套外部编译器运行时作为前置依赖。使用自有、针对 x86→Wasm 的轻量 IR；这是一项工程选择，不是宣称其他框架不能使用。

---

## 3. 前端：共享解码与全指令覆盖

### 3.1 DecodedInstruction

为每条指令保存：

```text
encoding_id
instruction_pc / next_pc
bytes / length
operand_size / address_size
prefixes / segment_override / lock / rep
operands：寄存器片段、立即数、EA 描述
instruction_class
可供分析阶段读取的控制流分类
```

Guest EIP 偏移、带 CS 基址的线性地址、代码物理地址和 Wasm 内存地址必须区分。不能把现有 `CpuContext.eip` 的使用方式直接当成全部 IR 中 PC 的定义。

解码器只读受控代码快照，不执行客户机访存 helper，不推进真实 CPU EIP，不触发客户机缺页。代码跨页且第二页不可供安全编译读取时，返回可识别的 compile-stop/无法取全指令结果；由执行路径在正确位置处理取指问题。完整解码器仍需能表示和测试跨页指令。

### 3.2 一份编码目录，不复制三套 opcode 表

沿用 `x86_table.js` 的编码目录和生成机制，按稳定 encoding_id 补充 IR 语义映射。编码元数据只描述事实，不要求把所有语义塞进巨大的 JS 数据表。[S2][S3]

生成器产出：解码分派、lowering coverage skeleton、非法编码项和编码形式清单。复杂语义由 Rust lift 函数实现。生成文件不手工修改。

### 3.3 全覆盖矩阵

每一个有效形式登记：

```text
encoding + 前缀 + 位宽 + reg/mem + 必要模式
→ native HIR | contracted helper | 基线不支持的明确行为
→ tests
→ 与基线模型有关的限制
```

迁移期间允许 `Pending`；正式默认切换前，基线已支持形式的 `Pending` 必须为零。

同时统计静态覆盖率与按实际执行频次加权的 direct-HIR/helper 分布，防止“所有指令都调用一个大 helper”被误报成高性能完成。

---

## 4. HIR 数据模型

### 4.1 类型系统

建议至少提供：

| 类型 | 用途 |
|---|---|
| I1 | 条件值，MIR 时降低为 i32 |
| I8/I16/I32/I64 | 显式位宽整数；窄类型的高位规范在 lowering 中明确 |
| F32/F64 | 明确浮点语义及允许映射条件的标量值 |
| V128 | SIMD 位向量；操作携带 lane 类型 |
| F80 | x87 语义值/位模式，使用软件表示，不假装 Wasm 有原生 F80 |
| FlagState | 按位来源和延迟求值表达 |
| Effect | 有序副作用依赖；编译期 token，不生成一个运行时变量 |
| Address/GuardProof | 区分 EA、线性地址、已验证 RAM 地址和对应 proof |

整数运算使用明确定义的截断/模运算语义；不借用会允许错误消除的未定义溢出。对 shift、rotate、除法、宽被除数、结果溢出与异常分别建模。Wasm trap 不能冒充客户机 #DE/#PF。[S15]

### 4.2 SSA 与 block 参数

推荐区域内 SSA + block 参数，而不是以 guest 寄存器号直接作可变临时变量。

维护 `GuestState -> ValueId/StateId` 映射。使用 sealed-block 算法或等价方法处理尚未确定的前驱与循环回边；提供 trivial phi 消除。多入口区域各自有合法状态初始化，不能让某入口依赖另一入口先执行。

在基本块内，寄存器读取返回当前 SSA 值，寄存器写入更新映射；到外部可观察点才按需要物化 CPU 状态。

### 4.3 寄存器别名

GPR 以 8 个完整 32 位寄存器为主值，片段操作表示为 extract/insert：

```text
AL  := low8(EAX)
AH  := bits8..15(EAX)
AX  := low16(EAX)
write AH(v) := (EAX & 0xffff00ff) | ((v & 0xff) << 8)
```

必须覆盖 16 位代码、地址回绕和 SS 的栈地址宽度；不能因只关注 XP 用户态就省略 BIOS/内核路径。

### 4.4 最低必要操作集

```text
Const, Add, Sub, Mul, WideMul, BitOp, Shift, Rotate,
Extract, Insert, Extend, Truncate, Compare, Select,
MakeFlags, ReadFlag, MergeFlags,
ComputeEA, GuestLoad, GuestStore, GuestRmw,
VecOp, ScalarFpOp, X87Op,
CallHelper, Guard, PollBudget,
Branch, CondBranch, IndirectExit, ReturnToDispatcher,
RaiseGuestException, StateTransition
```

其中常规访存、RMW、helper 和异常相关操作必须携带 source instruction、状态恢复点及有序 effect；不能都视为纯表达式。

---

## 5. FLAGS、状态恢复与精确异常

### 5.1 FLAGS 按位数据流

FlagState 为 CF/PF/AF/ZF/SF/OF 分别记录来源：常量、输入位、算术 recipe、选择/合并。系统控制位独立建模，不与算术标志一起 DCE。

必须覆盖的测试包括：ADD/ADC/SUB/SBB 的 carry、INC/DEC 保留 CF、旋转只改相关位、零移位计数保留状态、LAHF/SAHF/PUSHF/POPF、SETcc/CMOVcc、以及指令定义不完全的 flag 位。

对架构未定义位，维护基线允许的稳定行为，不引入 poison 语义。外部差分仅屏蔽确属未定义的位，不能把定义清楚但实现错误的位也屏蔽。

### 5.2 StateMap 是编译期信息，不是每条指令复制 CPU

每个可能退出或观察状态的位置拥有逻辑恢复描述，至少包含：

```text
instruction_pc / next_pc / resume kind
GPR mappings + FLAGS recipe
必要的 x87/SIMD 状态 mappings
已提交指令计数或预算账本
必要的 REP 进度
控制状态来源与 helper 适配类别
```

这些描述可使用共享父节点和 delta 压缩。只保存异常出口真的需要的值；快路径不因有 StateMap 就执行全寄存器写回。

Wasm 执行引擎不会替模拟器暴露任意位置的 guest 寄存器状态。需要在生成的冷分支中，根据 StateMap 显式生成恢复代码；不能仅存一张表然后指望宿主自动恢复。值被 state map 引用时仍算活跃，local allocator 不得提前复用。

只允许对纯、非访存、不会异常的表达式做出口重计算；不能为恢复状态重新读取可能变化的内存或再次执行 helper。

### 5.3 提交边界与多访问指令

采用“准备结果 → 完成必要访存/检查 → 按该指令定义提交状态”的 lowering 纪律，但不能把所有 x86 指令都假设为全有或全无事务。

例如：

```asm
add eax, 1
mov edx, [esi]    ; 可能发生缺页
cmp eax, ecx
```

即使正常路径上 `add` 的 FLAGS 后续被覆盖，`mov` 的异常路径仍需要能恢复 `add` 后的状态。

PUSH/POP、CALL、RMW、跨页写、字符串指令以及复杂系统指令必须各自定义提交点。REP 在部分迭代完成后退出时，保存准确 ECX/ESI/EDI 和继续执行位置，不能重复执行已经产生效果的迭代。

状态恢复、代码失效和必须重新评估中断的出口，是动态翻译器的核心约束；QEMU 的文档也明确把延迟状态恢复和此类出口作为专门机制。[S16]

---

## 6. Helper 契约与旧语义复用

### 6.1 HelperDescriptor

为每个可被 IR 调用的 helper 登记：

```text
id / signature / ABI
reads_state / writes_state
reads_memory / writes_memory / alias classes
may_fault / may_dispatch_exception / may_change_eip
may_change_mmu / segments / cpu_mode / fpu_mode
may_do_io / may_reenter_or_yield
may_invalidate_code
purity / determinism / observer classification
```

未知契约默认保守，禁止默认 pure。CPU 状态可被异常路径间接读取，也算读取。

与 QEMU TCG 的 helper 分类思路一致，读写状态、异常和无副作用属性要区分；“返回值未被使用”不意味着调用可删除。[S17]

### 6.2 Adapter 与异常所有权

不要假设现有所有 helper 都遵循相同失败约定。逐个审计并分组：

1. 纯返回值、不会异常的函数。
2. 返回失败，由调用方恢复状态并派发客户机异常。
3. 可能已经修改 EIP、进入异常处理程序或更新控制状态的 legacy helper。

第三类调用前必须同步其所有直接及间接观察状态；调用后若已发生控制转移，立即退出，不能再次恢复旧 snapshot 把异常处理状态覆盖回去，也不能重复派发异常。

建议使用编译期约定的 `Normal/FaultNeedsDelivery/ControlTransferred/Yield/Invalidated` 等 outcome。具体 ABI 可为返回标签或保守状态检查，不要求每次调用都分配 Rust enum。

### 6.3 可长期保留与不得长期保留

可长期保留：复杂系统指令、MMIO 慢路径、完整 x87/浮点特殊情况、复杂字符串边界处理。

不得以完成项目为名长期保留：普通 MOV/ADD/CMP/Jcc 全走通用解释 helper；整个基本块调用旧 `jit_instruction()`；IR 节点里面隐藏任意旧 `WasmBuilder` 片段。

---

## 7. 访存、分页与优化 proof

### 7.1 地址空间与 effect 域

区分 guest RAM、CPU 状态、MMIO/I/O、只读编译元数据及 host shared-memory 协议。alias 分析默认保守：guest 栈不是天然私有，两个不同线性地址也可能映射到同一物理页。

所有 may-fault、MMIO、写内存及 control state 操作初期使用保守 effect 顺序；只有证明不改变可观察行为时再细分并放宽。

### 7.2 HIR GuestLoad/GuestStore lowering

```text
EA / segment / size / privilege
  → 必要的分段检查和线性地址
  → 检查同页、TLB、权限和 RAM 属性
  → 快路径：受 proof 支配的 Wasm load/store
  → 慢路径：现有语义经 adapter 处理
  → 正常继续或按照正确 StateMap 退出
```

不能直接把 guest 地址当作宿主 Wasm 地址；不能让宿主 out-of-bounds trap 代替客户机异常。

### 7.3 GuardProof

复用地址转换需记录至少：页、权限、access size/range、映射/上下文版本、RAM/MMIO 属性和必要的 code-write 条件。

proof 必须支配其全部使用，且路径上无使其失效的 helper、权限/映射修改、控制状态变化。首次合法访问所需的页表副作用不能被编译期检查替代。

循环优化先复用无副作用的检查结果和地址计算。将整个范围检查提到循环前时，失败必须退回原顺序路径，不能提前触发客户机异常、访问 MMIO、推进设备状态，或遗漏原本会执行的更早指令。

### 7.4 载入/存储消除的限制

即使 load 的数据结果已死，只要它可能缺页、访问 MMIO 或产生被建模的副作用，就不能直接删除。

DSE、store-to-load forwarding、load CSE 需要同时证明别名、effect 顺序、访问有效性和观察点条件。禁止越过预算/中断安全点、未知 helper、设备调用或共享内存同步点直接传播。若某 RAM 区域存在 host/设备并发写入，必须按实际共享内存协议建模其可见性；“区域内没有 helper”本身不是内存值不变的证明。

LOCK/RMW 的原子和排序语义单独处理。即使当前为单 vCPU，也不能未经审计把 LOCK 退化成任意分离的普通 load/store。

### 7.5 字符串与 bulk 优化

显式保留 DF、REP/REPE/REPNE、位宽、分段和中途退出。重叠的前向 LZ 类复制具有迭代依赖，不可一律替换为 `memcpy` 或 `memory.copy`。批量优化只作用于已证明等价的区间；在页边界、设备访问和预算边界回到可恢复状态。

---

## 8. MMX/SSE/x87 全面迁移

### 8.1 SIMD

XMM 作为 V128 SSA 值贯穿区域；内存操作数可以直接得到向量值，不把每一步都经过 scratch memory。仅在真正观察点写回。

迁移当前已经验证的 SIMD 映射，同时逐指令审计：标量操作高位保留、不同 shift 规则、NaN 与 signed zero、MIN/MAX、转换/舍入、对齐故障、MXCSR 及基线支持的 FP 异常语义。[S8]

不能用 relaxed SIMD 或浮点重结合默认替代严格路径。无法证明等价的指令形态使用精确 helper；fallback 是该操作的正式 lowering，不是未实现标记。

MMX 与 x87 的别名、tag 状态及 EMMS 必须共享状态模型，不能当成两套互不相关寄存器。

### 8.2 x87

HIR 保留 X87Op、TOP、tag、控制字、状态字及 F80 值/位模式。普通内存加载与栈重命名可优化，算术先复用已验证软件语义，再迁移/扩展严格快路径。

禁止把所有 F80 转成 F64。SoftFloat 对扩展精度和舍入有专门语义；缩窄后再扩宽无法恢复已经丢失的信息。[S18]

游戏近似浮点属于另一个可选项目，不能作为本工程正确性的默认条件。

---

## 9. 优化 pass 管线

### 9.1 Tier 1：完整但低编译成本

```text
decode → HIR/SSA → cheap canonicalize → local const/copy propagation
→ conservative FLAGS demand → local DCE → MIR → Wasm
```

包含完整语义覆盖，但不要求为冷区域运行全部 CFG/循环分析。保留基线兼容运行时出口；不能因为 IR 编译预算耗尽而生成半个模块。

### 9.2 Tier 2：跨块/跨页热点区域

建议固定顺序并有收敛预算：

1. CFG 清理、支配/循环分析、trivial phi 消除。
2. 常量和 copy propagation、窄位宽 canonicalization。
3. FLAGS 使用需求、分支直接比较、按位来源合并。
4. snapshot-aware DCE 与可重计算值分析。
5. 副作用约束的 GVN/CSE、地址表达式复用。
6. helper 状态读写裁剪、跨内部边状态保持。
7. proof-based TLB/RAM 地址转换复用。
8. 保守 load forwarding / 冗余访问处理。
9. LICM、归纳变量和有限强度削弱。
10. SIMD lane/寄存器状态优化，受控循环模式优化。
11. 有限 CFG 简化、轻量 pass 再执行，随后 MIR lowering。
12. MIR guard 共用、local/栈优化、冷出口布局。

每个 pass 有开关、前置条件、变换计数、负例测试和 verifier。CFG、alias 或 dominance 失效后必须显式重算/更新，不能使用旧分析结果。

### 9.3 IR verifier 的硬性规则

类型和位宽正确；定义支配使用；block 参数/边实参数量一致；每块有唯一终结；effect 顺序完整；helper 契约存在；可能退出操作有恢复点；RAM proof 支配且有效；state map 引用保持活跃；优化不凭空新增可观察 fault；编译期元数据不进入 guest 快照。

### 9.4 初始区域预算

以下只是调参起点，不是已测最佳参数或性能承诺：

| 项目 | Tier 1 | Tier 2 |
|---|---:|---:|
| basic blocks 上限 | 8 | 64 |
| guest instructions 上限 | 128 | 512 |
| HIR nodes 上限 | 1,024 | 8,192 |
| 代码依赖页上限 | 2 | 8 |
| 生成 Wasm body 预算 | 64 KiB | 256 KiB |

还要限制 peak live locals、StateMap 总大小、pass 迭代次数、结构化复制量和编译 work units。命中任一预算即收缩区域/停止扩展，而不是让浏览器承担无限模块。测试后按真实编译成本调整。

---

## 10. Wasm 后端

### 10.1 MIR 与后端独立性

前端和 HIR passes 不允许直接调用 WasmBuilder。MIR 降低之后，明确了具体 helper ABI、快慢访存路径、异常出口、向量映射和状态写回。

复用 WasmBuilder 的二进制编码能力，但旧 builder 的 deferred FLAGS 和旧 JitContext SIMD/RAM caches 不得与 IR 优化重复管理同一状态。IR 路径中只有一个状态真相来源。

### 10.2 SSA 消除与结构化

先处理 critical edges 和边上的 parallel copy；循环复制使用临时 local 打破，不能按错误顺序覆盖值。

结构化可适配当前 `control_flow.rs` 的策略，但接口应使用 IR CFG。复制 block 时同步复制/重写边参数、source maps 和恢复信息。不可约 CFG 保留 dispatcher 正确路径，不要求所有 CFG 都变成完美的结构化循环。[S9]

### 10.3 Local 分配与栈调度

做类型感知的 liveness/coalescing/local reuse；把可单次使用且无副作用的表达式安排在 Wasm 操作数栈，其他值放 locals。快慢路径和 StateMap 的活跃性必须一起计算。

IR ValueId 不等于永久的 Wasm local index。一个节点分配一个永久 local 会造成变量膨胀和编译压力。这里是虚拟 locals 的分配，不是直接控制浏览器使用哪一个宿主物理寄存器。

### 10.4 外部 ABI

先保持现有模块调用框架兼容，通过统一出口状态完成与解释器、JS、设备和旧模块的切换。区域内部尽量不物化，区域外部必须有明确契约。

guest CALL/RET 必须保持客户机栈和控制流语义，不能未经专门设计就映射成宿主 Wasm 的调用栈。区域入口状态、间接跳转和客户机返回地址不服从普通宿主函数 ABI。

真正的 tail-call chaining 是可选扩展，不作为 IR 项目完成的必要前置条件。不默认要求所有浏览器支持额外 Wasm 特性；SIMD/可选特性均做能力检测，并在相同 IR 管线内降级。

---

## 11. 运行时、链接、代码失效与发布

### 11.1 CompileRequest / Artifact

建议接口表达如下；这是设计签名，不是现有 API：

```text
compile_region(
  request: CompileRequest,
  code: ImmutableCodeSnapshot,
  config: IrConfig
) -> Result<CompiledArtifact, CompileError>
```

请求包含：request id、VM/reset generation、tier、入口集合、CpuModeKey、代码页快照/版本及预算。产物包含：Wasm bytes、入口映射、依赖页、退出信息、编译统计和对应 generation。

编译器读取请求快照，不临时依赖执行过程继续变化的真实 CPU 寄存器。

### 11.2 CpuModeKey

当前 `CachedStateFlags` 包含 is_32、SS32、CPL3、flat segmentation 等信息。[S19]

凡被 IR 编译期当常量使用的额外状态，都必须进入 key、guard 或明确的失效依赖。不能在未扩展验证契约的情况下，把全部 CPL、CS/段属性、CR0/CR4、分页模式、A20、FP 模式等当成隐含不变。

key 不应粗暴包含所有 CPU 状态：没有特化的状态无需入 key。区分可复用的物理代码翻译与受映射/地址特化约束的 entry/link cache。

### 11.3 热度与升档

普通入口与链接入口都能贡献热度。进入某热区域即记账，但实际升档在没有 guest locals 存活的安全位置排队处理。不能在嵌套执行中销毁/替换仍被依赖的元数据。

当前链接与 performance recording 有路径交互，升档测试必须包含 recording 全关、链接开启的情形；不要仅测试开 profiler 后的路径。[S11]

### 11.4 失效依赖图

维护 `physical code page → regions` 和 `region → dependent entries/links`。覆盖：guest store、DMA/设备写、host write_blob、相关共享内存写入口、页映射更新、控制模式变化、snapshot restore/reset。

地址转换 cache 与 code bytes cache 的失效条件不同，分别建模。页表写入如何影响硬件可见转换语义，要沿用客户机 MMU 模型，不能不分条件地把所有页表 store 都当成全局 TLB flush。

当前执行区域被 self-modifying store 影响时，清除未来入口还不够：必须在继续执行可能过期的后续指令前退出或有明确证明允许继续。

### 11.5 发布与槽位复用

当前 JS 通过异步 Wasm 实例化安装函数；Rust 也已有“编译期间代码被写”的处理。[S6][S12]

新设计延续并增强这一契约：

```text
Queued → Building → Instantiating → Validated → Published
                         ↘ Cancelled / Stale / Failed
Published → Invalidating → Retired → Reclaimable
```

安装 table 前再次校验 job id、slot generation、VM generation 和代码依赖；过期结果必须丢弃，不能先覆盖复用槽位再发现错误。保持 pending/reserved 槽不对查找可见。

已有执行栈或链接引用可能仍在使用的对象，不得立即复用其元数据。按现有单 CPU 执行模型设立 quiescent point；未来多 worker 情形另行建立明确同步协议。

编译失败、Wasm validation 失败、浏览器实例化异常、预算拒绝等均有可统计退出；保留 Tier 1 或解释执行，不崩溃、不留半发布记录、不无限重试同一失败。

### 11.6 预算、安全点和设备

循环回边、长 REP、区域出口与控制状态变化设置预算检查。被 DCE 删除的模拟维护代码，不应让客户机逻辑指令计数/调度成本凭空消失。

RDTSC、I/O、HLT、STI/POPF、MOV/POP SS 的相关中断语义和基线支持的调试观察点应保留。不能因为单线程就把设备交互跨安全点重排。

graphics proxy 共享内存提交和 PCI/MMIO 操作视为外部可观察边界。沿用该设备协议的可见性与同步约束，不让 IR 重排“命令写入”和“提交通知”。

---

## 12. 可按依赖执行的 PR / 工作包

每个工作包提交代码、测试、验证结果和文档；不能只交付设计文件或空模块。无工期承诺，依赖顺序如下。

| ID | 工作包 | 依赖 | 主要交付与验收 |
|---|---|---|---|
| IR-00 | 固定基线与测试驱动 | 无 | 记录 SHA、构建/运行配置、原始成绩；后端选择骨架；测试不能改变正式路径 |
| IR-01 | WasmBuilder 泛化 | 00 | u32/LEB 索引、签名和容量检查；边界模块 validation+执行通过 |
| IR-02 | 共享 decoder 与覆盖目录 | 00 | decoded 结构、生成器、全形式目录；长度/前缀/EA/边界差分测试 |
| IR-03 | HIR/MIR/SSA 基础 | 01,02 | arena、block 参数、effect、dump、verifier；循环、多入口、错误图测试 |
| IR-04 | 状态恢复与 helper ABI | 03 | StateMap、异常所有权、保守 helper adapters；故障点/重复派发测试 |
| IR-05 | 首批完整整数 lowering | 04 | GPR/片段、算术/逻辑/FLAGS、移位、乘除、条件分支；不是最终只做这一批 |
| IR-06 | RAM/MMU/栈/RMW | 04,05 | 快慢路径、段/页检查、原子与提交点；缺页、别名、跨页测试 |
| IR-07 | 控制/字符串/系统指令 | 06 | CALL/RET、REP、I/O、模式变化、复杂语义 helpers；完整覆盖矩阵更新 |
| IR-08 | MMX/SSE/x87 全覆盖 | 04,06 | SIMD SSA、FP lowering、F80 helpers、别名与控制状态；特殊值与模式测试 |
| IR-09 | 完整 Wasm 后端与 Tier 1 | 05–08 | phi/结构化/local/出口；Tier 1 语义覆盖完整、可运行系统；无旧 emitter 嵌入 |
| IR-10 | Tier 2 基础数据流优化 | 09 | FLAGS/DCE/GVN/copy/CFG/helper-state passes；正例/负例/逐 pass 差分 |
| IR-11 | 访存和循环高级优化 | 10 | proof-based 地址复用、受控 LICM/forwarding/SIMD；异常顺序负例通过 |
| IR-12 | 区域、缓存、链接、发布 | 09；设计始于04 | generation、依赖失效、安全升档、编译取消；SMC/映射/槽复用/恢复压力测试 |
| IR-13 | 全测试矩阵与端到端调优 | 10–12 | release/浏览器/宿主矩阵；冷热分离；图形、输入、音频、加载兼容验证 |
| IR-14 | IR 默认与旧发射路径退役 | 13 | IR Tier1/Tier2 成为默认；生产构建无旧 emit 调用；文档、覆盖与回滚报告 |

说明：后端和 runtime 要从 IR-03/04 起建立最小可执行脚手架，不能真的等到 IR-09 才第一次生成 Wasm。IR-09 指完成其全能力与 Tier 1 覆盖；早期各指令包必须通过同一逐步完善的后端测试。

并行边界：IR-01 与 02 可并行；契约冻结后，整数/系统/SIMD/x87 lowering 可以分工。SSA、effect、StateMap、helper ABI 和发布协议必须统一设计，不能各模块自行发明。

关键依赖不能颠倒：没有可验证 StateMap，就不启用跨 fault point 的 FLAGS 删除；没有代码依赖失效，就不扩大跨页区域；没有 RAM proof，就不启用访存/循环提升。

---

## 13. 测试与验收矩阵

### 13.1 四层差分

1. 固定输入状态：解释器 vs legacy JIT vs IR Tier 1 vs IR Tier 2。
2. HIR 参考执行器/变换前后执行：定位 pass 错误。
3. 适用的指令和模式：现有 QEMU/宿主测试交叉验证，注意模型/未定义行为差异。
4. OS 与真实应用：发现仅在分页、设备和时间交互下出现的问题。

共享 helper 的差分不是独立数学正确性证明；重点 helper 仍需独立测试。随机测试保存 seed、完整初始状态、页映射、指令序列与最小化后的失败样本。

确定性差分使用固定输入、调度和设备事件；不能让同一个 MMIO 读在两个后端上真实执行两次，再把设备差异误判为 CPU 错误。可在受控设备模型上重放事件。

### 13.2 必须包含的边界类别

| 类别 | 重点用例 |
|---|---|
| 解码 | 66/67、重复/组合前缀、ModRM/SIB、16/32 EA、非法形式、跨页取指、不同入口 |
| GPR | AH/AL/AX/EAX 交错、截断、符号扩展、地址/栈回绕 |
| FLAGS | carry 链、INC/DEC、count=0、条件码、PUSHF/POPF、未定义位策略 |
| 算术异常 | 除零、宽被除数 quotient overflow、精确 PC 与提交状态 |
| 普通访存 | guard 失效、只读/不存在页、用户/内核模式、对齐和跨页 |
| 别名/RMW | 同物理页不同虚拟地址、重叠字节访问、LOCK、代码页写 |
| 指令提交 | PUSH/CALL/RMW 故障前后的 ESP/GPR/FLAGS；多访问部分完成 |
| REP | DF、零计数、提前终止、第二页故障、预算中断、重叠迭代复制 |
| 控制流 | 菱形、phi cycles、不可约 CFG、多入口、间接目标变化 |
| SIMD/FP | lane/高位保留、特殊值、转换、舍入、MMX-x87 别名、F80 模式 |
| helper | 只读/写状态、观察 FLAGS、触发故障、已派发异常、控制模式改变 |
| JIT 生命周期 | 编译中代码变更、当前 region 自修改、slot 复用、异步过期回调 |
| 映射与恢复 | CR3/INVLPG 等基线支持的失效路径、reset、snapshot restore |
| 调度/设备 | 长循环不饿死设备、I/O 次数顺序、共享内存提交协议 |
| emitter | >127/>255 索引、长度/签名、locals 类型复用、parallel copy |

### 13.3 现有测试复用

仓库已有 assembly、QEMU/KVM、JIT paging、OS boot、API 和 expect 测试体系。[S14]

当前核对到的构建目标包括：[S13]

```bash
make all
make nasmtests nasmtests-force-jit jitpagingtests qemutests rust-test
make cpu-optimization-tests flags-provenance-tests jit-tiers-tests
make softfloat-fast-tests x87-fast-tests mmx-fast-tests
make jit-capacity-tests cpu-plan-tests cpu-experimental-policy-tests
make api-tests
```

这些命令仍需要仓库规定的依赖、镜像和环境。重构中将适用的测试参数化到各后端，不在一次性能比较中混用不同 JS/Wasm 版本。

### 13.4 拟新增目标与配置

以下名称需要实现后才可运行，不是当前已有命令：

```text
make ir-decoder-tests
make ir-verifier-tests
make ir-semantics-tests
make ir-differential-tests
make ir-exception-tests
make ir-invalidation-tests
make ir-backend-tests
make ir-workload-benchmark
```

拟新增配置：

```text
jit_backend = legacy | ir
ir_opt_level = 0 | 1 | 2
ir_verify = off | debug | every_pass
ir_dump = off | hir | mir | wasm | all
ir_passes_disabled = [...]
ir_region_budget = {...}
ir_stats = off | sampled | debug
```

配置需要沿实际初始化入口和 CPU Worker 消息路径（若启用）透传；构建产物中写入 backend/build-id。最终 production 默认 ir，legacy 只保留在对照构建或回滚版本。

---

## 14. 性能验证与发布门槛

### 14.1 指标

分别记录 decode/lift/pass/lower/emit/浏览器编译安装耗时、Wasm 字节、locals 峰值、StateMap 大小、编译次数、重复编译与失效、helper 调用、RAM 快慢路径、区域出口、Tier 2 执行覆盖和端到端耗时。

区分总墙钟时间与内部采样，不把 profiler 下改变了运行路径的比例当成正常 release 的比例。调试计数可影响性能，应单独量化。

### 14.2 工作负载

固定同一 XP 镜像和同一 Everest 5.50 配置，分别测 Queen/PhotoWorxx/ZLib/AES。用户已给分数仅作为待复现记录，不从中反推出目标倍率。

另外测 WZ/RHO 本地加载、固定地图进入时间、固定游戏场景的 CPU 帧时间与尾延迟。3D 场景同时记录 GPU/帧上限，避免把 GPU 瓶颈误作 CPU 优化无效。

每次比较严格配对相同宿主、浏览器、客户机 RAM/磁盘状态、CPUID、线程数、计时路径和游戏设置。至少多次重复并报告中位数/波动；首次冷启动、保存状态恢复后的首次运行、同一实例热运行分开。

### 14.3 两类完成条件

**功能完成：** 全覆盖目录、全部强制正确性测试、稳定发布/失效、两层 IR 默认执行、无生产旧发射调用。

**性能完成：** 根据真实矩阵另行判定。建议把可重复且超出噪声的主要工作负载回退列为发布阻断；例如超过 5% 的持续回退必须调查并明确处理。5% 是建议工程阈值，不是硬件规律。

不能以“IR 功能完成”声称已达到 Core 2 Duo。达到该目标必须指定具体 CPU 型号、单线程条件、软件版本和实际成绩；也不能保证任意应用都有同一加速比。

编译开销计入首次加载总时间。某个 pass 热跑加速但首次加载明显退化时，应调整分层/预算，而不是隐藏冷启动数据。

---

## 15. Definition of Done

- 固定基线的全部已支持编码形式有实现、契约与测试；Pending 为零。
- Tier 1、Tier 2 正常生产编译统一经过 IR；常用直接映射族无通用逐指令解释退化。
- SSA/effect/StateMap/proof verifiers 完整，关键 pass 有正负例和差分。
- 每类 guest fault/部分完成/外部观察点使用正确状态，未出现错误回放或重复异常派发。
- 自修改、页别名、模式变化、在途编译、slot 复用、快照恢复均通过压力测试。
- Wasm 编码不依赖隐含小索引上限；特性缺失有明确降级。
- 现有 CPU、分页、OS、API、图形代理和设备行为无新增已知回归。
- 报告 release 的冷热性能、编译成本、内存与回归，不声称未验证的倍率。
- 默认切换可回滚；旧 JIT 从生产发射路径退役，保留可复现的基线版本。

---

## 16. 可交给编码代理的任务说明

```text
任务：在 speedyHKjournalist/v86 实现完整 IR JIT，遵循本计划 IR-00 至 IR-14。
最终覆盖 Tier 1 与 Tier 2，不以热点原型或仅添加数据结构为完成。

先记录实际 HEAD、工作区状态和基线构建配置，不覆盖现有未提交改动。
按依赖顺序实施，每阶段包含可执行实现、正负例测试、验证记录及文档更新。
允许迁移期间双后端对照，但最终生产路径不得调用旧指令发射器。

必须先建立共享 decoder、SSA/effect、精确状态恢复、helper 契约和
WasmBuilder 索引/编码能力，再启用跨块 FLAGS、访存和循环优化。
复杂指令可保留精确 helper，但不能用通用 InterpretOne 或旧 JIT emitter
绕过全部语义建模，且普通高频指令必须迁移到直接 HIR lowering。

保留 guest CPU/设备/快照对外兼容；不顺手添加 SMP、x86-64 或更改 graphics
proxy 协议。默认不启用近似浮点，不更改 CPUID/计时制造跑分提升。

每次交付报告：实际修改文件、已运行命令、通过/失败结果、尚未执行的测试，
以及当前覆盖矩阵。未运行 XP/游戏时不得声称兼容或提速已经验证。
```

---

## 17. 参考来源与溯源

仓库来源是本次读取的 master 关键文件；实施时应把文档中的引用转换成固定 SHA 的永久链接。来源仅支持现有结构/语义约束；新增架构、目录、接口、PR 顺序和预算均是本计划的设计选择。

| ID | 来源 |
|---|---|
| S1 | 仓库 README：`https://github.com/speedyHKjournalist/v86` |
| S2 | 编码目录：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/gen/x86_table.js` |
| S3 | JIT 生成器：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/gen/generate_jit.js` |
| S4 | 编译上下文：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/cpu_context.rs`；寻址：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/modrm.rs` |
| S5 | 分析：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/analysis.rs` |
| S6 | JIT 管理：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/jit.rs` |
| S7 | 公共发射：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/codegen.rs`；调用链由 S3/S6 核对 |
| S8 | SIMD：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/simd_codegen.rs` |
| S9 | 控制流：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/control_flow.rs` |
| S10 | WasmBuilder：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/wasmgen/wasm_builder.rs` |
| S11 | CPU 执行/链接：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/cpu/cpu.rs` |
| S12 | JS 安装模块：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/cpu.js` |
| S13 | 构建与测试：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/Makefile` |
| S14 | 测试说明：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/tests/Readme.md` |
| S15 | Wasm 数值语义：`https://webassembly.github.io/spec/core/exec/numerics.html` |
| S16 | QEMU 翻译器约束：`https://www.qemu.org/docs/master/devel/tcg.html` |
| S17 | QEMU TCG/Helper：`https://www.qemu.org/docs/master/devel/tcg-ops.html` |
| S18 | SoftFloat：`https://www.jhauser.us/arithmetic/SoftFloat-3/doc/SoftFloat.html` |
| S19 | 当前状态 key：`https://raw.githubusercontent.com/speedyHKjournalist/v86/master/src/rust/state_flags.rs` |
