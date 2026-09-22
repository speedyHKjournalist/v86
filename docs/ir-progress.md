# IR-00–IR-14 实施状态

## 2026-09-23 设计、功能与实际 XP 镜像复核

本轮以 `7591db21` 为基线，对照实施计划逐项核查 IR-00～14，并使用实际
`windowsxp_multidisk_C_4G.img` 做关闭插桩的交替配对。新增 x87/MMX 寄存器
helper 续执行、双目标 owner 链接、完整字节块比较、直接 continuation context
核对、HIR/MIR 位集活跃性与 local 分配，以及纯计算段的冗余 epoch 检查消除。
融合 snapshot 的重叠验证去重仍逐字节保留全部代码和映射证明。修复 I/O
权限回调的过晚快照、OUT 的旧 EAX、单次 INS/OUTS 的回调状态写回，以及
debug SSE/MMX 日志观察点的解码/续执行语义。热度阈值调整为 64/256，默认
热度容量保留 128；扩大到 512 的 XP 负向实验也留档。
后续交叉审查补齐普通 SSE/MXCSR/显式 SSE 非法形式的 debug 冷退回及
非法 MXCSR 的日志观察点，并将缺失入口提示扩为 64 个完整 key 槽。
所有变换保留精确退休、异常恢复、代码身份与任意宿主回调的检查契约。

最终 XP 四臂三轮中位数：旧 IR **47.089 mIPS / 32.998 秒** → 新 IR
**64.417 mIPS / 29.013 秒**（吞吐 +36.8%，耗时 −12.1%）；legacy 为
**126.873 mIPS / 19.587 秒**。终点仅为首次 800×600×32 显示模式。
原生测试 271 passed，差分、缓存/预算/无 SIMD 和真实浏览器集成回归通过；
等量指令的 core/vector/fp_helpers 性能门槛仍未全部满足。

完整设计审查、阶段性和最终测量、回归验证及未完成事项见
[2026-09-23 审查报告](ir-review-20260923.md)。**超过 legacy 的 XP 性能目标尚未达到，
IR-14 默认迁移不能验收。** 下方历轮的“完成”仅适用于各自明确限定的工作包，
不表示一般 helper 状态裁剪、跨模块寄存器传递、完整模式组合与 OS 工作负载均已完成。

## 2026-09-22 设计与热路径审查

本轮固定基线为合并 PR #52 后的 `e4622847788c68213e9d4c34fd15853fe2bcf0c6`。
新增普通 RAM 向量写后续执行（保留物理代码别名/故障/MMIO 出口）、16 类 SSE
基本 FP 寄存器快路径、可关闭且保持精确退休/恢复的 owned-MIR 预算批处理，以及
无编译/发布工作时的调度负向 hint。公开 pass 名称增至 18 类，新增 `budget_batch`。

同核三轮配对中，向量专项较旧 IR 提升约 2.15–12.43 倍；但 core/vector 两个性能
门槛均未全部满足，**XP 超过 legacy 的目标未验证，完整请求尚未完成**。没有修改
默认后端、执行预算、CPUID/TSC 或严格浮点策略。935 编码、3972 粗粒度形式中仍有
3728 production Pending；experimental Pending 为 0 不等于完整生产验收。

逐 IR-00～14 审查、实现安全边界、原始性能数据与复现入口见
[2026-09-22 审查报告](ir-review-20260922.md)。下方内容保留为历轮实施记录；其中
基线、覆盖数和 pass 数对应其记录时间，不应覆盖本节的新状态。

## 历轮实施记录

更新：2026-09-20。固定基线：`8ee73e538daaab15411344d39a1f271e778ac7f3`。

**完整请求尚未完成。** 本次落地了可执行的实验性 IR 编译器与 WasmBuilder
前置改造。默认后端仍为 legacy；显式选择 IR 时已有自动编译和升档，
尚未完成纯 IR 的 Windows XP 启动验收。
原始验收范围以 [实施计划](v86-ir-implementation-plan.md) 为准，没有降低其完成条件。

| 工作包 | 状态 | 已落地及剩余工作 |
|---|---|---|
| IR-00 | 部分完成 | 已固定 SHA、保存 release 基线、建立测试入口、编译器配置类型及初始测量记录；已有公开 legacy/ir 后端选择、区域参数、Worker 传递和状态查询；新增公开 ir_opt_level 与 17 类 pass 禁用、实际编译路径验证和页面参数保留；已补 ir_verify/ir_dump/ir_stats 的公开配置、真实 Worker 与复制读取接口；完整工作负载验收仍待完成 |
| IR-01 | Builder 改造已实现 | u32 locals、完整 LEB 索引/长度、结构签名、类型复用及真实模块执行边界测试；在线 table 容量仍是运行时策略 |
| IR-02 | 部分完成 | 生产区域分析已消费共享 decoded，2,678,271 例与旧 analyzer 一致；解释器与快照 decoder 已共享前缀和 ModRM/SIB 规则，新增 71 个缺失分组和显式非法形式 CPU 路径；已验跨页取指/空段优先级，剩余完整状态组合仍须工作负载验收 |
| IR-03 | 部分完成 | HIR arena、SSA、effect、支配/verifier、dump、可执行 dispatcher、边复制和 local 复用；已将标量/RMW 读取和 XMM 访存的 guard、物理动作、CPU 调用与退出策略迁入 MIR 计划；已增加 RMW 提交、地址/范围检查和 SSE 守卫的 effect 计划；已加入 CMPXCHG8B/除法的边界、寄存器与提交计划及通用 helper 调用点的观察/暂存/出口计划；已加入 lowering 拥有的 dispatcher CFG 和 typed 边复制调度；已加入标量/向量值程序、packed kernel 选择及机器栈类型检查；已加入有序状态物化与独立计数阶段，以及 SSA 动态计数基数；已移除保留的 HIR 副本，加入受校验的 lowering 事务、独立 MIR 所有权和机器常量折叠；已增加 HIR 丢弃后的受预算约束栈融合、typed local 后置分配及边复制重建；已加入不可变快照多入口拆分及独立守卫；更广泛的 MIR 图变换和其余动作仍待实现 |
| IR-04 | 部分完成 | StateMap、helper 副作用及异常所有权、可执行 outcome ABI；42 例 Wasm 验证状态物化、快照槽位复用和单次派发；已适配 CPU 分段/安全访存及真实 #PF/#GP；已增加终端 CPU 状态 helper 契约；byte frontend 的 CPU CallHelper 已有集中签名/outcome registry；已增加 CpuReload 正常返回时的 GPR/FLAGS/XMM 重载及 CFG 续执行；已登记 31 个低层 import 并校验类型/返回协议；续执行审计已包含描述符、TR/LDTR 和控制 FLAGS；其他 ABI 仍须按引入范围审计 |
| IR-05 | 部分完成 | 寄存器算术、FLAGS、别名、条件码、移位/旋转、SHLD/SHRD、乘除法及终端分支可生成 Wasm；last_op1 和原始 ZF 来源已进入 SSA/快照；已增加位测试/修改、位扫描、POPCNT、BSWAP、XADD、CMPXCHG、BCD、FLAGS 传送、符号扩展和计数分支；已增加可达直接 CFG 字节 lifting；已接入 TEST alias、ARPL 等剩余粗粒度形式及有界在线区域选择；完整模式/前缀与多入口验收未完成 |
| IR-06 | 部分完成 | MOV/moffs/XLAT、整数 ALU/比较/条件操作及 INC/DEC/NEG/NOT 访存；成对 RMW ticket、原生 RAM 和精确 MMU/MMIO；已增加 PUSH/POP、PUSHA/POPA、LEAVE、ENTER 和多次访存提交；已增加交换指令、原生 RAM CMPXCHG8B 及非共享单线程 ABI 下经审计的 LOCK；已增加 FLAGS/段栈操作、段 MOV 和远指针加载；通用 proof 及其余访存待实现 |
| IR-07 | 部分完成 | 近 CALL/RET、FF /2 与 /4 间接转移及动态 EIP StateMap；已增加单次 MOVS/CMPS/STOS/LODS/SCAS 原生执行；已增加标量 IN/OUT 和单次 INS/OUTS；已接入有界 REP HIR、进度映射及最终提交；已增加 CPUID/RDTSC/RDMSR/WRMSR 终端适配；已增加 SYSENTER/SYSEXIT、HLT/CLI/CLTS/WBINVD；已增加 CR/DR 传送与 CPU 地址映射变更适配；已增加描述符表、SMSW/LMSW 与 INVLPG；已增加 SLDT/STR 与 LLDT/LTR；已增加 LAR/LSL、VERR/VERW；已增加远 CALL/JMP/RETF、INT/INTO/IRET 的终端 helper；已增加 STI shadow 与嵌套 STI 的不可分割片段；新增 VM86、调用/任务门、任务切换、跨特权 IRET 和多访问故障对照；保留基线 panic/未实现分支，完整 OS 系统验收仍未完成 |
| IR-08 | 部分完成 | XMM V128 SSA、快照、typed locals/边复制，以及 packed/scalar SIMD 传送的原生 RAM 与精确慢路径已实现；已增加 38 种 packed integer 算术/比较/乘法/逻辑及 PS/PD 逻辑别名、打包/解包与 packed 移位、PSHUF/SHUF、半部传送、MOVD/MOVQ、lane、MASKMOVDQU 等；新增 D8-DF x87 寄存器形式的 terminal CpuX87Helper，复用 canonical F80 CPU 语义并显式处理 CR0.EM/TS #NM、legacy x87 cache barrier 与 nested #UD。已增加 x87 memory/environment、FXSAVE/FXRSTOR 与 MXCSR；保留基线不支持形式。已增加 274 个 MMX 和 272 个 SSE FP/conversion 粗粒度形式；已扩展 F80/F32/F64 特殊值与控制模式矩阵，修复 IR/解释器 NaN payload 内联分歧，接入无 SIMD 便携核心与解释器降级；仍待完整工作负载/性能验收 |
| IR-09 | backend/Tier-1 infrastructure 完成 | IR 自有 Wasm backend 已形成完整路径：typed locals、MIR parallel-copy/phi edge、StateMap/预算/故障出口统一发射；新增通用 single-entry reducible CFG structurer，以 SCC/唯一循环入口递归生成 Wasm Block/Loop，nested branch/nested loop 均不再依赖逐 case detector，多入口/irreducible SCC 才使用 IR 自有 pc-local dispatcher fallback；`src/rust/ir` completion gate 禁止引用 legacy `jit_instruction`/`jit_instructions`/`codegen`/`control_flow` emitter。自动 Tier 1/2 均使用共享 decoder 的 bounded reachable-CFG region formation 和同一 HIR→MIR→Wasm backend；Tier 1 固定一轮低成本 prune/merge/phi/copy canonicalization，Tier-2-only FLAGS/GVN/DCE/helper-state/LICM/RAM forwarding 保持隔离。IR-09 对当前可 lowering 语义的 backend/Tier-1 职责完成；实验粗粒度 Pending 已清零，但 IR-05～IR-08 的完整前缀/模式/异常及 OS 验收仍待完成，不计入 IR-09 backend 完成声明。详见 ir09-completion.md |
| IR-10 | 完成 | Tier 2 基础数据流优化已按实施计划的 FLAGS / DCE / GVN / copy / CFG / helper-state 六类验收收口：CFG 清理、trivial phi、常量与独立 copy propagation、逐位 FLAGS demand/CPU liveness、StateMap-aware DCE、支配关系 GVN/CSE、经审计 pure helper 的 CPU state observation trimming 均有独立开关/计数、正例/负例与逐-pass differential；特殊 FLAGS 或读取/写入 CPU state 的 helper 继续保守 materialize，不猜测未证明状态。IR-11 的 RAM proof/forwarding、LICM、强度削弱和高级 SIMD/循环优化不计入 IR-10 |
| IR-11 | 完成 | Tier 2 已收口受预算约束的纯 SSA LICM、整数 SIMD peephole、owned-MIR RAM 证明与 fault-preserving memory LICM：地址关系按 Exact/Disjoint/MayAlias 保守分类，已证明不相交的 native 标量 store 可保留既有 load cache；自然循环采用唯一 preheader 重置的 loop cache，首次访问仍在原故障点执行且只有成功 native RAM guard/load 才建立有效值，任一慢速 guest-memory 路径在 MMIO/page-walk 前清空全部 loop cache；含 store/RMW/vector memory/未知 helper/effect 的循环不做 memory LICM。证书由 verifier 独立重算，伪造/预算失败不会部分发布。详见 ir11-completion.md |
| IR-12 | 完成 | 区域/缓存/链接/发布生命周期已按计划收口：不可变请求与 VM generation、物理页 dependency/mapping identity、非回绕 publication/slot owner、三阶段发布重校验、冷点回收与活动帧延迟释放形成统一失效协议；自动 Tier1/Tier2 调度保持 128 heat/单 pending/单帧扫描上限，失败输入抑制并以单调 use stamp 做确定性自动入口淘汰；新增 cache-owned validated link lookup，只对精确 CpuEntryKey 返回当前候选，并重新检查 generation、bytes、mapping identity；cached-TLB 可见性仍由实际执行 admission 检查，绝不作为 unchecked call_indirect；SMC、reset/restore、映射变化、浏览器失败、ABA 槽复用和同步 I/O 失效均由 lifecycle matrix 覆盖。IR-core 现强制运行 ir-live/cache/auto tests。详见 ir12-completion.md |
| IR-13 | 完整矩阵未完成 | host/browser/device acceptance gate 与同核 execution-budget 矩阵已建立。#46/#47 已确认 structured CFG 显著降低 reducible hot-loop 内部 dispatcher 开销；IR-09 completion 进一步把 backend 推到通用 reducible structurer 与 Tier-aware region formation，并由 IR-13 smoke 持续回归。XP、真实应用/游戏、稳定冷热性能阈值，以及 IR-05～IR-08/剩余 ISA 覆盖回补仍未完成；因此尚不能进入 IR-14 默认 backend/legacy emitter 退役 |
| IR-14 | 未实现 | 默认后端仍为 legacy，旧 emitter 未退役 |

覆盖目录共 864 条编码记录、3,830 个粗粒度形式，其中 102 个是明确的
baseline-UD reg/mem 形式，**3,728 个生产形式仍为 Pending**。
当前 3,728 个形式有实验 native/helper 或显式基线行为，experimental Pending 为 0。
本批新增 950 个形式，涵盖远控制流、STI、x87 memory、FP-state、SSE FP、MMX、
TEST alias 及其余保留/非法/基线形式；具体类别见 [覆盖目录](ir-coverage.md)。
这些数字不表示所有前缀、特权、子编码组合均已逐项验收，也不表示新增实现了基线缺失 ISA。

`make ir-default-gate` 在生产 Pending 非零时按预期失败。`ir-experimental`
Cargo feature 允许 IR 入口参与 CPU 分派，并提供可选的自动编译/升档策略；
该策略默认关闭，生产默认后端仍为 legacy。
没有宣称 XP 兼容、游戏加载改善或 IR 提速。

下一依赖：继续完善 IR-02/03/04 契约、更广泛 MIR 变换、完整 ISA 模式矩阵与无 SIMD 降级。
下一步以 IR-13 acceptance matrix 暴露的问题为入口，反向补 IR-02～IR-09 的 decoder/ISA/helper/Tier-1/系统覆盖缺口，并继续 XP、应用与冷热性能验收；
可选的自动 IR 策略不等于完成生产默认 Tier 的全面迁移。

详见 [测试报告](ir-validation.md)、[实现说明](ir-design.md)、
[helper 契约状态](ir-helper-contracts.md) 和 [覆盖说明](ir-coverage.md)。

## 后续推进：公开后端与 CPU Worker

- `jit_backend: "ir"` 现在可通过 V86 构造参数进入主线程或真实 CPU Worker，开启
  自动 IR 编译并关闭 legacy 生成。默认仍为 legacy；IR 未支持的语义由解释器执行。
- `ir_region_budget` 校验整数范围和未知键；`get_jit_info()` 返回有效配置和复制的统计，
  Worker 仅传回可克隆数据。性能报告增加实际后端标签，继续保留已有 Wasm SHA-256 字段。
- 无效选项或缺少 IR 的内核通过 `emulator-error` 拒绝，发生在 guest autostart 之前。
  初始化最终异步回调现在接回 Promise 错误链。`disable_jit` 同时关闭两个生成器，
  已修复关闭 legacy 生成后已有 Tier 1 仍可能发起升档的问题。
- Node debug/release 和 Chromium 主线程/Worker 均通过同一公开 API 场景：Tier 1/2、
  SMC、x87 解释回退、save/restore/restart、legacy/IR 双向快照及错误拒绝；IR 场景的
  legacy 编译请求数均为零。完整 ISA、其余配置开关、系统和性能验收仍待完成。
- 配置和测试入口见 [公开后端说明](ir-backend.md)。

## 后续推进：独立 MIR 所有权

- `MirRegion` 已移除 HIR 副本，独立拥有机器类型、local 分配、动作/值/helper 计划、
  dispatcher CFG 和状态物化计划。编译请求在 MIR 优化及 Wasm 发射前显式释放 HIR。
- 新增借用 HIR 的 lowering 事务；完成时核对既有 canonical 契约、机器类型与 local
  分配，才生成只读 MIR 产物。原有错误计划测试仍保留，但拒绝点提前到产物构造边界。
- 独立 MIR 常量折叠处理 i32/i64 字面量机器算术、位操作、移位、比较、位计数、类型
  转换和 select。替换由自有类型表校验、批量提交，有明确 work 上限，不移动读取、
  helper、状态物化或预算恢复点。三个优化编译入口均已接入并记录 `mir_folds`。
- 新增 HIR 销毁后发射、错误类型/分配、观察保留、预算失败原子性及编译入口测试；
  独立 oracle 已执行 12,768 次字面量 Wasm 对照和 36 次窄寄存器 MIR 改写。
- 这完成了 MIR 的独立产物生命周期，不代表通用可变 MIR 图 verifier、图变换、
  MIR 后置 SSA 分配或栈调度完成。详见 [MIR 所有权说明](ir-mir-owned.md)。

## 后续推进：共享生产分析前端

- `analysis::analyze_step` 对完整已知编码使用共享 decoder 的控制流事实；编译上下文的
  物理代码地址与逻辑 EIP/线性地址分开传递。未知形式暂保留固定基线的分析回退。
- `CpuContext::instruction_snapshot` 只复制同页已分配 RAM，不调用 MMIO，不推进客户机
  EIP；缺失第二页在只读 decoder 中返回 compile-stop。生产 JIT 仍沿用页尾区域截断。
- 通过真实 Wasm CPU 中的 2,670,035 例对照，覆盖 16/32 位、ModRM/SIB、组合/重复前缀、
  EA 字段、长度、条件分支、STI、块边界及生产适配接口。测试导出仅进入 `ir-test-hooks` 构建。
- 差分找出并修复了两个先前遗漏：非 custom ModRM helper 的块边界；`0F 7C/7D` 无强制
  前缀形式在基线中先取 ModRM、再进入无 EA 的非法形式。这些修复没有改变原解释器语义。

## 后续推进：可执行 helper ABI

- 已适配的 i32 helper 可从 HIR 降为明确的 MIR 导入签名及 outcome 冷出口，再生成 Wasm。
  未适配、需要正常路径状态 reload、缺失故障派发器或签名冲突的调用会拒绝编译。
- 调用前写回当前 StateMap；返回值先暂存，正常路径才写入 SSA local。调用方故障恢复后
  只派发一次；已转移、yield、invalidated 保留调用后的权威状态并直接退出。
- 42 例优化前后 Wasm 测试使用仪表化 callee，包含返回值与快照 local 复用及非法 outcome。
  这验证了 ABI 执行路径，尚不是通过真实 CPU/MMU 派发客户机 #PF/#DE 的验证。

## 后续推进：真实 CPU 访存

- `compile_cpu_region` 可生成使用实际 CPU GPR/FLAGS/IP/MMU 的冷入口产物，尚未安装到
  在线 Tier/cache。普通构建仍为 legacy。详见 [访存契约](ir-memory.md)。
- 192 例 MOV 访存差分、32 例已确认原生 RAM 路径、20 例真实 #PF/#GP、12 例 MMIO
  回调比较通过；也覆盖非零 CS、lazy FLAGS、重复状态物化计数和自修改代码别名。
- 首批 store 明确携带成功 commit map，每次成功写入后退出；不会继续执行可能已过期的
  后续指令。在线失效接入后才能按依赖证明保留 store 后的区域执行。

## 后续推进：整数访存与 RMW

- 内存 ALU/CMP/TEST、INC/DEC/NEG/NOT、SETcc、CMOVcc 已进入 CPU HIR lowering。
  RMW 先验证全部写页、再读数据；纯 SSA ALU 计算新值和 FLAGS，最后消费物理地址 ticket。
- verifier 保证 ticket 单次配对使用，禁止跨 effect、块参数传递或普通计算；i64 ticket
  有独立类型与 local。慢写前物化新 FLAGS，故障路径保留旧 StateMap。
- 2,040 例整数访存、340 例原生暖 RAM、36 例真实异常、340 例 MMIO 对照通过。
  另外验证设备读回调重映射、非连续物理页和 false-CMOV 仍需读取内存；跨页 CR2 的
  对齐查询差异已按固定基线修正。详见 [访存契约](ir-memory.md)。

## 后续推进：PUSH/POP

- 寄存器/立即数 PUSH、寄存器 POP、FF /6 和 8F /0 的寄存器/内存形式已接入原生
  SSA 指针计算及既有 RAM/MMU/MMIO 路径。栈宽度与操作数/地址宽度独立处理。
- 864 例普通路径、432 例原生暖路径、128 例回绕/ESP 高位边界、100 例真实异常及
  80 例 MMIO 对照通过；ring3 PUSH 的故障通过真实 TSS 切换到独立内核栈验证。
- 保留固定基线的 5C 与 8F POP SP 高位差异，以及 POP 内存段故障的临时 ESP 调整。
  这些是明确的基线语义适配。完整说明见 [栈契约](ir-stack.md)。

## 后续推进：近 CALL/RET 与间接跳转

- StateMap 增加经过类型/支配验证的动态目标 EIP，参与 DCE、替换和 local 活跃范围。
  CALL 保存目标后压入返回地址；RET 成功读栈后才提交 EIP 和 ESP。
- 1,632 例近转移、48 例动态目标边界、200 例指令内异常、6 例后续取指异常和
  192 例 MMIO 对照通过；涵盖 CALL ESP、CALL [ESP] 和返回地址覆盖目标源的情况。
- 近控制仍以终端冷入口产物执行。远转移、系统/REP 和在线链接没有因此完成。
  详见 [近控制契约](ir-control.md)。

## 后续推进：PUSHA/POPA、LEAVE 与指令内部写入

- `GuestCheck` 显式预检整个栈范围，保持首尾页故障次序且不触发设备读写。
  `PartialStore` 执行 PUSHA 前七次写入，最后一次 store 才提交并退出；verifier
  要求同一客户机指令、连续 effect 和匹配的最终提交，禁止把部分写入带入下一条指令。
- POPA 的七次读取各有进度快照，跳过保存的 SP 槽；LEAVE 的寻址栈宽度独立于操作数宽度。
  每次后续访问重新检查翻译，设备重映射不能复用先前的页权限结果。
- 栈套件扩展为 960 例普通对照、480 例暖数据访问、128 例 MMIO；新增 72 例真实故障、
  16 例跳过 SP 槽、144 例回绕、8 例自身代码别名和 32 例回调重映射对照。
  ENTER、FLAGS/段寄存器栈操作、生产集成及完整 IR-00～IR-14 仍未完成。

## 后续推进：移位、旋转与 FLAGS 来源

- C0/C1、D0～D3 及 SHLD/SHRD 已用纯 SSA 实现；内存形式使用既有 RMW ticket，
  计数为零也保留基线的写权限检查及设备读写。没有通过整条解释器 helper 执行算术。
- 为保留基线未定义 AF 的行为，`last_op1` 成为经过验证的 FLAGS 来源，参与状态快照、
  DCE/local 活跃范围及 CPU 写回。整数算术退出后接着运行解释器移位也能保留这个来源。
- 746,688 例 CPU/逐位参考模型对照、50,208 例暖 RMW、2,976 例真实故障、672 例 MMIO
  与 672 例 lazy-FLAGS 慢路径通过；覆盖全部 256 个寄存器 CL 输入及优化前后产物。
  详见 [移位契约](ir-shifts.md)。生产 Pending 仍为 3,728。

## 后续推进：原生宽乘除与 i64 后端

- MUL/IMUL 及全部 DIV/IDIV 宽度已进入 HIR；显式 IMUL 寄存器/立即数/内存形式一并支持。
  乘法使用原生 i64，除法先检查零除、i64 特例及客户机商范围，再提交商和余数。
- #DE adapter 只负责真实异常派发，不执行整条算术指令；源操作数 #PF/#GP 优先于 #DE。
  129,500 例成功执行、16,900 例单次 #DE、688 例访存异常及 24 例 ring3/TSS 帧对照通过。
- 后端补齐普通 I64 标量、局部变量、块间并行复制和混合宽度 helper ABI。RMW ticket
  仍保持独立仿射类型。完整契约见 [乘除法说明](ir-multiply.md)；在线 CPU 循环与生产发布仍未接入。

## 后续推进：位字符串、扫描、POPCNT 与 BSWAP

- BT/BTS/BTR/BTC 使用原生 SSA，内存形式按基线只访问选中的一个字节；寄存器索引
  有符号扩展后调整地址，不会重新按 address16 截断。修改形式使用字节 RMW ticket。
- BSF/BSR 保留零源时的旧目的寄存器，并按基线维护未定义 FLAGS 来源；POPCNT 使用
  强制 F3 解码形式。新增 I32/I64 原生位计数及一致的常量折叠，BSWAP 使用纯 HIR。
- 54,720 例普通对照、393,216 例 16 位扫描/计数穷举、480 例故障及 384 例地址边界通过。
  实验覆盖增加 60 个 NativeHIR 和 44 个 CpuMemoryHIR 形式。详见 [位操作契约](ir-bits.md)。

## 后续推进：交换、条件交换与显式 LOCK 顺序

- XCHG/XADD/CMPXCHG 的寄存器和内存形式使用原生 SSA，覆盖高字节别名和隐含累加器。
  CMPXCHG 比较失败仍执行基线内存写周期，XADD 在源/目的相同时保持正确提交次序。
- RMW load/store 显式携带 Plain/Locked，verifier 校验成对顺序一致；内存 XCHG 隐含 Locked。
  已审计非共享 Wasm 内存、单一 CPU 执行线程和同步调用，接受有限的合法 LOCK 内存形式；
  共享内存、重入 CPU 执行和多核原子性不在此 ABI 中。详见 [交换与 LOCK 契约](ir-exchange.md)。
- 48,384 例普通对照、20,736 例暖路径、5,616 例真实故障、1,296 例 MMIO、864 例跨页成功，
  以及 576 例 LOCK 算术/一元/位修改对照通过。实际共享内存导入被拒绝，同步调度检查通过。
  实验覆盖增加 12 个 NativeHIR 和 18 个 CpuMemoryHIR 形式；生产 Pending 仍为 3,728。

## 后续推进：ENTER 与指令内故障进度

- ENTER 的全部 5 位嵌套深度使用有界 SSA 展开，每次帧链读取和压栈都有独立快照。
  保留基线先执行嵌套复制、最后写入旧 BP 的次序；分配栈空间只调整指针，不额外访存。
- 显式建模基线嵌套访问在异常派发后的宿主中止，以及 ENTER16 向 MMIO 传递未截断帧值。
  这些行为以 release CPU 为差分基线，未宣称 debug 断言行为等价。详见 [ENTER 契约](ir-enter.md)。
- 1,600 例普通对照、800 例暖原生路径、384 例回绕/帧别名、192 例 MMIO、32 例重映射、
  16 例自身代码别名及 48 例真实 ring3 故障通过；其中 32 例检查派发后宿主中止。
  verifier 检查全部 256 个原始嵌套立即数和策略字段，实验栈覆盖增加至 104 个形式。

## 后续推进：标量转换、FLAGS、BCD、moffs 与 XLAT

- CBW/CWDE、CWD/CDQ、SAHF/LAHF、SALC、CLD/STD 和全部 BCD 调整进入原生 SSA。
  AAM 支持全部立即数基数，零基数复用单次 #DE 派发；DAA/DAS 未定义 OF 显式采用保留输入策略。
- moffs 和 XLAT 通过既有 RAM/MMU/MMIO 路径，覆盖独立的地址/操作数宽度、段覆盖及 16/32 位回绕。
- 88,032 例普通标量对照、3,292,160 例穷举、378 例组合序列，以及 3,136 例隐含访存、
  1,048 例访存异常、392 例 MMIO、84 例 AAM #DE 和 16 例 ring3/TSS #DE 通过。
  实验覆盖增加 28 个 NativeHIR、14 个 CpuMemoryHIR 和 2 个 CpuArithmeticHIR 形式。
  详见 [标量与隐含访存契约](ir-misc.md)；生产 Pending 仍为 3,728。

## 后续推进：LOOP/LOOPcc、JCXZ/JECXZ 与相对分支出口

- 计数器宽度由地址大小决定，跳转目标宽度由操作数大小决定；LOOP 的减一不修改 FLAGS，
  CX 形式保留 ECX 高位，JCXZ/JECXZ 不修改计数器。Jcc/JMP 与计数分支共用相对出口构造。
- 138,240 例 CPU 对照、69,120 例独立 ABI 执行、1,048,576 例 CX 穷举和 432 例 SSA 组合通过。
  64 例目标取指 #PF 验证分支和计数器先提交、随后异常保留已完成状态。
- 实验终端分支覆盖增至 152 个形式；覆盖 JSON 的测试路径已按指令族指向真实测试套件。
  完整 CPU 循环区域、动态计数和在线链接仍待实现。详见 [计数分支契约](ir-loops.md)。

## 后续推进：FLAGS/段寄存器栈与终端 CPU helper

- PUSHF/POPF 和段寄存器 PUSH/POP 使用原生栈访问及经过审计的状态适配器。
  保留 PUSH 段寄存器的“预留四字节、仅写两字节”，以及 POP SS 按新栈宽度调整指针的次序。
- 新增 CpuExit helper 契约：指令完成计数由适配器负责，调用后直接保留 CPU 权威状态；
  不允许正常继续或恢复旧快照。POPF 启用 IF 后立即进入 IRQ 的状态已通过真实 PIC/TSS 验证。
- 864 例普通对照、416 例回绕、208 例 MMIO、480 例选择子检查、104 例 ring3 栈缺页、
  80 例描述符缺页、156 例实模式、32 例 VM86 和 16 例即时 IRQ 通过；另有权限、页边界和 ABI 负例。
  实验栈覆盖增加至 156 个形式。详见 [系统栈与终端 helper 契约](ir-system-stack.md)。

## 后续推进：段 MOV、远指针加载与描述符提交

- 8C/8E、LES/LDS/LSS/LFS/LGS 已接入原生操作数访存及终端描述符适配器。
  明确区分 selector 的两字节宽度与目标寄存器宽度，描述符校验成功后才提交远指针 GPR。
- LinearOffset 表示分段解析后的线性偏移，远指针尾部不重新按地址16截断，也不复用首次读取的权限证明。
  两次读取之间发生 MMIO 重映射时，第二次读取按新映射执行。
- 9,344 例普通对照、3,104 例 ring3 操作数故障、624 例描述符故障、816 例 MMIO、
  1,952 例边界、640 例重映射、612 例实模式和 1,224 例 VM86 对照通过，另有选择子校验负例。
  实验访存覆盖增至 618，新增 28 个 CPU 状态形式。详见 [段传送契约](ir-segments.md)。

## 后续推进：单次字符串原生执行与提交顺序

- MOVS/CMPS/STOS/LODS/SCAS 非 REP 形式进入原生 SSA 与 RAM/MMU/MMIO 路径。
  DF、16/32 位指针回绕、段覆盖、源先于目标的数据访问和故障前旧指针均已建模。
- 3,360 例普通对照、1,200 例完整 FLAGS 数据边界、240 例 MMIO、264 例数据故障、
  112 例 ring3 第二页故障及 96 例回调重映射通过，另覆盖段检查、物理别名、自修改代码和实模式/VM86。
- 新增 30 个实验单次字符串形式；REP 不计作已实现。其分页批次、比较 FLAGS 时点、
  部分进度和预算仍需独立契约。详见 [字符串实施说明](ir-strings.md)。

## 后续推进：端口 I/O、TSS 权限与设备观察点

- IN/OUT 和非 REP INS/OUTS 接入显式操作数与终端 I/O 适配器，权限/设备/异常仍由 CPU 负责。
  OUTS 源读取走原生访存；INS 保留端口读取前的完整写权限检查及回调后的实际写入检查。
- 6,336 例普通 CPU/设备对照、2,688 例权限组合、480 例位图位检查、720 例 TSS 故障、
  272 例字符串故障次序、144 例 MMIO、48 例回调重映射和 144 例实模式对照通过，另覆盖 DF/页边界。
- 修正正常返回的 CPU helper 的指令 IP 观察点：调用前暴露 decoded next IP，previous_ip 保留故障位置。
  新增 36 个实验 I/O 形式；REP 与在线调度仍待推进。详见 [I/O 契约](ir-io.md)。

## 后续推进：有界 REP 执行器与显式进度结果

- 共享字符串引擎新增 Complete/Repeat/Fault 结果及实际迭代数，早期故障和部分完成故障不再靠 EIP 猜测。
  快路径和慢循环都支持元素预算；零计数先于预算、段检查和设备访问。
- 旧入口保留原有批次策略。独立参考构建从固定基线提取旧字符串函数体，隔离验证此次共享 CPU 改动。
- 1,344 例旧/新对照、588 例预算、288 例提前终止、252 例故障及最大计数器、LZ 物理别名、重入测试通过。
  该接口不更新 instruction_counter；REP HIR/StateMap、调度工作量及最终指令提交的接入仍待实现。
  详见 [REP 执行器契约](ir-rep-engine.md)，未据此增加已实现的 REP 覆盖。

## 后续推进：REP HIR、元素工作量与最终提交

- 七类 REP 指令接入 CpuRep 终端 ABI；StateMap 显式记录与 GPR 映射一致的 ECX/ESI/EDI 进度。
  Yield 和故障保留 CPU 权威状态，只有最终完成才提交一次 REP 指令。
- 独立元素预算进入 CompileRequest 配置与 helper 参数，运行时结果记录本次元素数和退出原因。
  测试使用不同的首次/续执行入口，确认前缀不会重复执行，部分批次不冒充完成指令。
- 14,112 例普通对照、672 例有界重入、1,176 例故障/页重入、384 例提前终止及预算/设备/权限测试通过。
  编码表中的 F2/F3 独立条目新增 84 个实验 REP helper 形式；在线调度与代码代际验证仍待接入。
  详见 [REP HIR 契约](ir-rep.md)。

## 后续推进：CPUID、时间戳和 MSR

- CPUID/RDTSC/RDMSR/WRMSR 进入具名终端 CpuExit 适配器，保留 CPU 的特性、计时和 MSR 策略。
  CPL/CR4.TSD 失败只派发一次 #GP；成功后计数一次并保留 CPU 权威状态退出。
- debug/release 各通过 3,920 例 CPUID、2,870 例 MSR、224 例确定性 TSC、280 例权限、
  28 例实模式、42 例最大 CPUID leaf 配置及四组连续 TSC 读写；各另有 32 例未知 MSR/APIC 限制验证。
- 时间源仅由测试实例注入，生产计时未改变；共有语义 helper 的对照只证明集成一致性。
  详见 [CPU 信息与 MSR 契约](ir-cpu-info.md)。新增 8 个 CpuInfoHelper 形式，生产 Pending 仍为 3,728。

## 后续推进：系统模式切换与 HLT 观察点

- SYSENTER/SYSEXIT、HLT、CLI、CLTS、WBINVD 使用具名终端 CpuExit；成功后保留 CPU 的
  段缓存、CPL、执行宽度、FLAGS、EIP/ESP 和取指状态，不恢复旧 SSA 快照。
- 672 例普通对照、3,024 例权限/VME、504 例选择子/FLAGS、168 例实模式、252 例目标边界、
  56 例成功提交后取指 #PF、112 例 HLT 计时器/停机/IRQ、84 例异常 MMIO 观察以及
  56 例无描述符访存的快速系统转移通过。VME 异常派发保留基线宿主中止，不宣称新增 VME 支持。
- 新增 12 个 CpuSystemHelper 形式。STI shadow、其余系统/ISA、在线调度/发布和 XP 验收仍待实现。
  详见 [系统转移与 HLT 契约](ir-cpu-system.md)；生产 Pending 仍为 3,728。

## 后续推进：CR/DR 传送与地址映射提交

- MOV CR/DR 的四类具名终端适配器使用完整 32 位 GPR，保留 ignore_mod 解码规则；
  CPL 校验先于非法 CR、DR4/5 别名与 CR4.DE 校验。成功与已派发异常显式区分，不通过 EIP 猜测。
- debug/release 各通过 7,680 例普通传送、1,792 例权限/VM86、64 例 DR 别名故障、
  128 例非法 CR、64 例 CR4 位检查、168 例实模式及 14 例预热 TLB 保留/失效对照。
- 各通过 60 例 RAM/MMIO PDPTE 加载和部分中止、96 例 PDPTE 位策略、18 例 CR0/CR3
  基线边界，以及四例 CR3 成功提交后取指缺页。debug 断言和已有宿主中止均保留基线行为。
  详见 [CR/DR 与映射契约](ir-control-regs.md)；新增 8 个 CpuControlRegHelper 形式。
- 已有 CPU TLB 维护不等于在线 IR 发布/失效已接入。STI shadow、其余系统/ISA、完整 MIR/区域、
  在线 Tier、优化、XP/性能验收及旧 emitter 退役仍待实现，生产 Pending 仍为 3,728。

## 后续推进：描述符表、机器状态字与 INVLPG

- SGDT/SIDT/LGDT/LIDT、SMSW/LMSW、INVLPG 进入具名终端适配器，EA/分段在 HIR 中
  显式计算；复合表访问使用 CPU 的安全访存，未宣称新增原生 HIR 访存覆盖。非法寄存器形式明确派发 #UD。
- 六字节存储先预检，再按原时点读取并写入两个表字段；加载只在两次读取成功后提交。
  MMIO 回调改变映射或表 base 时维持既有顺序，晚期写故障保留基线派发后宿主中止。
- debug/release 各通过 5,040 例普通对照、280 例权限、448 例访存故障边界、560 例空段、
  168 例地址尾部边界、48 例 base 掩码、56 例 MMIO、32 例重映射、32 例晚期故障/中止、
  16 例延迟读取 base、224 例实模式及 16 例 INVLPG 目标/全局 TLB 失效与相邻条目保留。
- 新增 56 个 CpuDescriptorHelper 形式，含显式非法寄存器分支；生产 Pending 仍为 3,728。
  详见 [描述符与 INVLPG 契约](ir-descriptor.md)。其余 ISA、完整 MIR/区域、在线 Tier/发布/失效、
  高级优化、XP/性能验收与旧 emitter 退役仍未完成。

## 后续推进：任务寄存器与 LDTR

- SLDT/STR、LLDT/LTR 进入具名终端适配器；模式检查先于特权和数据访问，内存形式的
  EA/分段检查仍在 HIR 中。新增 load_tr_checked 明确返回描述符读取故障，旧 void ABI 保留。
- LTR 先更新 TR 缓存，再写 busy 位；写故障保留新 TR、已派发异常和基线宿主中止。
  独立参考构建将解释器 LTR/LLDT 函数体固定为基线 SHA，IR 使用当前函数体。
- debug/release 各通过 1,440 例普通对照、128 例模式/权限、128 例源操作数故障、160 例空段、
  160 例空/越界选择子、640 例分批独立 CPU 描述符属性、8 例 TI 选择子、32 例 MMIO、48 例描述符读/只读、
  16 例 busy 写故障/中止、32 例回调重映射及 32 例物理描述符尾部/独立 busy 写翻译。
- 新增 32 个 CpuTaskRegHelper 形式，详见 [任务寄存器契约](ir-task-regs.md)。生产 Pending 仍为 3,728；
  当时尚待实现的 VERR/VERW、完整任务切换与其余 ISA、MIR/区域、在线 Tier/生命周期、XP/性能验收和旧 emitter 退役仍待实现。

## 后续推进：LAR/LSL 与 FLAGS 观察状态

- LAR/LSL 已接入终端查询适配器，区分源 #PF 与描述符 #PF；后者保留基线在异常派发后
  写回原目的值的行为，包含 ESP 和 16 位高半部保留。MMIO 源读取后才捕获原目的值。
- debug/release 各通过 1,920 例普通对照、64 例模式、320 例无效选择子、8,192 例独立
  type/DPL/RPL/CPL/present/conforming 矩阵、32 例源故障、64 例描述符故障、80 例空段、
  64 例 MMIO、32 例读取后目的值捕获及 64 例 LDT 查找。新增 16 个 CpuSelectorQueryHelper 形式。
- 当时 VERR/VERW 尚待实现：八例可复现实验证明基线在描述符 #PF 时保存 raw ZF，而非此前 INC
  的计算 ZF。`make ir-flags-observer-tests` 已保留该证据；需补充 FLAGS backing-state 契约，
  不能直接套用普通 StateMap 或掩掉该差异。详见 [选择子查询契约](ir-selector-query.md)。
- 生产 Pending 仍为 3,728；完整 ISA、MIR/区域、在线 Tier/生命周期、优化、XP/性能验收及旧 emitter 退役未完成。


## 后续推进：VERR/VERW 和原始 ZF 状态

- 在 FlagState、StateMap、verifier、优化替换和 Wasm 物化中加入独立 raw_zero 来源。
  CPU 出口保留原始 ZF，并用仅 ZF 延迟计算的规范表示保留可见算术 FLAGS。
  SAHF、BSF/BSR、POPCNT 显式更新 backing bit；普通算术等保留它。
- VERR/VERW 使用终端命名 helper，保留源读取之前的模式检查，以及描述符读取之前
  清除 flags_changed.ZF 的顺序。描述符 MMIO 和 #PF 保存基线原始 ZF。
- debug/release 每套通过 720 个普通形式、8,192 个独立权限组合、源/描述符缺页、
  分段优先级、MMIO 和 LDT 检查；另通过 8,640 个融合 IR、IR→解释器、
  IR→IR→解释器状态衔接检查。详见 [选择子契约](ir-selector-query.md)。
- 新增 16 个实验形式，CpuSelectorQueryHelper 合计 32；生产 Pending 仍为 3,728。
  全部 IR-00–IR-14 验收条件仍未满足，在线 Tier、完整 MIR/ISA、XP 和性能工作继续待实现。
- 全套回归通过：70 个 warnings-as-errors Rust 测试、全部已接入 IR 差分、
  独立基线 REP/任务寄存器套件及八例原始 ZF 诊断（`build/ir-verr-suite.log`）。
  Wasm feature 编译、生成目录、普通构建导出与 diff 检查通过。


## 后续推进：CMPXCHG8B 和 ZF 延迟标记

- 新增终端 CompareExchange8B HIR、类型/状态 verifier 和原生 i64 RAM 路径。
  写权限、用户权限、MMIO、代码页及跨页 guard 将慢路径交给命名 CPU adapter。
- 保留解释器的写预检、读取后捕获比较寄存器、匹配时写入、失配时无写入、
  最后清除 ZF 延迟标记的顺序；晚到 #PF/host abort 保留回调和部分写入状态。
- 新增 zero_is_lazy SSA/快照来源，修复 IR 返回解释器后 CMPXCHG8B 的 MMIO ZF
  观察差异；保留同页 MMIO 低双字符号扩展与跨页零扩展的基线区别。
- 3,584 个夹具覆盖模式/宽度/地址寄存器/段/前缀组合；实验访存形式合计 622。
  实现与证据范围见 [CMPXCHG8B 契约](ir-cmpxchg8b.md)。生产 Pending 仍为 3,728。
- 全套回归通过：72 个 warnings-as-errors Rust 测试和全部已接入 IR/独立基线套件。
  debug/release 每套完成 21,504 个 CMPXCHG8B 场景、21,504 次原生 RAM 入口，
  以及边界、权限、MMIO、回调改写、部分异常与 ZF 跨后端观察测试。
  Wasm feature 编译、目录/导出/diff 检查通过，日志为 `build/ir-cmpxchg8b-suite.log`。


## 后续推进：XMM SSA 和 SIMD 传送

- 接入 V128 local、并行边复制、XMM StateMap、lane 类型检查及优化 liveness。
  MOVUPS/MOVUPD、MOVAPS/MOVAPD、MOVDQA/MOVDQU、MOVSS/MOVSD 在寄存器和
  原生 RAM 路径中保留向量 SSA；只有观察、故障和退出才写回 CPU。
- EM/#UD、TS/#NM 检查先于 EA/段解析，保留 ModRM 后的准确观察 PC；保留基线
  OSFXSR 和未对齐行为。慢访存完成当前传送后退出，保留设备回调修改的 CPU 状态。
- 新增 64 个 CpuSimdHIR 实验形式；7,680 个夹具覆盖标量高位、寄存器别名和段覆盖。
  详见 [SIMD 传送契约](ir-simd-moves.md)。IR-08 的 MMX、向量算术、FP 控制和 x87
  仍未完成，生产 Pending 仍为 3,728。
- 全套回归通过：74 个 warnings-as-errors Rust 测试及全部已接入 IR/独立基线套件。
  debug/release 每套通过 11,264 个 SIMD 传送场景、7,168 次原生 RAM 入口、向量链、
  冷退出/重入、coalesced 边复制和脏 XMM 故障退出。Wasm feature、目录、导出及
  diff 检查通过；日志为 `build/ir-simd-moves-suite.log`。


## 后续推进：packed XMM 整数运算

- 新增 typed PackedOp、纯 VectorBinary 和有序 XmmBinary；38 种整数运算与 8 种
  PS/PD 逻辑编码别名直接生成 Wasm SIMD，保留向量 SSA 和原有 FLAGS。
- 内存原生路径继续 SSA；慢路径完整读取源后再捕获目标 XMM，保留 MMIO 回调改写、
  宽读取分解、页权限及异常状态。成功只提交当前指令后退出，故障不写回陈旧快照。
- 新增 184 个实验形式，CpuSimdHIR 合计 248；22,080 个夹具覆盖全部寄存器和段形式。
  debug/release 每套通过 32,384 个指令场景、5,888 个独立 BigInt 模型场景及权限、
  MMIO、重映射、脏 XMM 故障和续执行测试。详见 [packed 整数契约](ir-simd-integer.md)。
  生产 Pending 仍为 3,728；完整 IR-00～IR-14 尚未达到验收条件。
- 全套回归通过：77 个 warnings-as-errors Rust 测试、全部既有 IR/独立基线套件和
  新增 packed integer 差分套件，日志为 `build/ir-packed-full-suite.log`。
  Wasm feature 编译、最终契约测试、目录归属、普通导出隔离和 diff 检查也已记录。


## 后续推进：SIMD 打包、解包及 packed 移位

- 新增 19 种打包/解包/变量移位操作与 4 种 PS/PD 解包编码，以及 10 种立即数
  移位形式。纯向量节点直接发射 Wasm SIMD，计数按完整 u64/imm8 处理。
- 新增 VectorShuffle 类型/索引校验；PSRLDQ/PSLLDQ 通过字节重排及零填充实现。
  零计数仍保留 SSE guard、准确 PC 和指令提交。
- 差分发现并修正 UNPCKLPS/UNPCKLPD 的基线 8 字节读取区别。XmmBinary 与
  adapter 现携带显式宽度；新增上半页缺失测试，防止过读或错误省略必要的读取。
- 新增 112 个实验形式，CpuSimdHIR 合计 360，生产 Pending 仍为 3,728。
  [实现和验证范围](ir-simd-permute.md)记录了 33,120 个 packed 夹具和 14,720 个
  立即数夹具，以及独立模型、异常、权限、回调和计数边界测试。
- 全套回归通过：79 个 warnings-as-errors Rust 测试及全部已接入 IR/独立基线套件。
  混合向量链也通过冷退出/续执行和脏 XMM 故障检查；最终日志为
  `build/ir-packed-permute-full-suite.log`。目录归属、普通导出隔离和 diff 检查通过。
- 最终 Wasm feature 编译检查通过：`build/ir-packed-permute-check.log`。


## 后续推进：立即数 SIMD 重排

- 新增 PSHUFD、PSHUFLW、PSHUFHW、SHUFPS 和 SHUFPD。寄存器使用纯 VectorShuffle，
  普通 RAM 使用完整 16 字节读取与原生 byte shuffle，保留 V128 SSA。
- 新增显式 XmmShuffle 访存契约和命名 CPU adapter；完整读取源后采样目标，保留
  回调修改和异常状态。穷举覆盖 imm8，包括 SHUFPD 忽略的高位及 PSHUF 的源半部复制。
- 新增 20 个实验形式，CpuSimdHIR 合计 380；29,400 个夹具覆盖立即数、别名、模式
  和段组合。详见 [重排契约与证据](ir-simd-shuffle.md)。生产 Pending 仍为 3,728。
- 回归暴露了预期 host panic 的测试实例栈耗尽：实测单次调用可留下 12KB 以上栈帧。
  已仅在 task reference 测试产物导出栈指针，并隔离每次捕获的 host trap；正常返回
  必须保持栈平衡，客户机状态比较未削减。完整 task 套件已通过，生产代码未改动。
- 当前完整回归矩阵分两段通过：81 个 warnings-as-errors Rust 测试及前半套件见
  `build/ir-simd-shuffle-full-suite.log`；修复测试栈隔离后，task 与全部后续套件见
  `build/ir-simd-shuffle-resume-suite.log`。新重排数据、MMIO、故障和续执行检查通过。
- 最终 Wasm feature 编译、目录、普通导出隔离和 diff 检查通过；编译日志为 `build/ir-simd-shuffle-check.log`。


## 后续推进：XMM 半部传送、MOVD/MOVQ 和重复 lane

- 新增 15 个编码、48 个合法实验形式，CpuSimdHIR 合计 428。寄存器和 RAM 路径保留
  V128 SSA；MOVD 与 GPR SSA 直接衔接，MOVQ 和 MOVD 按基线清零高位。
- 新增有序 XmmTransferLoad；内存读取完成后采样保留半部，保持设备回调状态。
  XmmStore 显式记录源 lane，原生高半部存储和慢路径使用同一取值规则。
- 5,664 个夹具通过 debug/release 独立模型、别名、随机 GPR、精确访问范围、MMIO、
  故障、脏 XMM 和续执行检查。见 [传送契约与证据](ir-simd-transfer.md)。生产 Pending
  仍为 3,728，完整 IR-00～IR-14 验收条件尚未满足。
- 全套回归在单次 make 调用中通过：83 个 warnings-as-errors Rust 测试及全部已接入
  IR/独立基线套件，日志为 `build/ir-simd-transfer-full-suite.log`。目录归属、普通导出
  隔离及 diff 检查通过；测试栈隔离修复也在本轮完整矩阵中再次验证。
- 最终 Wasm feature 编译检查通过：`build/ir-simd-transfer-check.log`。


## 后续推进：XMM 符号位、word 插入/提取和剩余常规传送

- 新增 9 个编码、20 个合法实验形式，CpuSimdHIR 合计 448。VectorBitmask 与 16-bit
  VectorExtract/Replace 保留 XMM/GPR SSA；PINSRW 内存节点仅读取两字节。
- 慢路径完成读取后才采样目标 XMM，保留设备回调对其他 word 的修改；非临时存储
  和 LDDQU 复用已验证的 16 字节传送契约及基线对齐策略。
- 13,944 个夹具通过 debug/release 独立模型和 CPU 比较。每种构建穷举 65,556 个
  符号位掩码，并验证立即数屏蔽、精确页边界、MMIO、异常、脏 XMM 和混合链续执行。
  详见 [word lane 与掩码契约](ir-simd-lane.md)，聚焦日志为 `build/ir-simd-lane-suite.log`。
- 生产 Pending 仍为 3,728。完整 ISA/MIR、在线 Tier 发布失效、host fallback、XP 与
  性能验收，以及旧路径退役仍未完成；本次增量未改变这些验收状态。
- 全套回归在单次 make 调用中通过：85 个 warnings-as-errors Rust 测试及全部已接入
  IR/独立参考套件，日志为 `build/ir-simd-lane-full-suite.log`。最终 feature 编译见
  `build/ir-simd-lane-check.log`；目录归属、普通导出隔离和 diff 检查通过。


## 后续推进：MASKMOVDQU 有序掩码存储

- 新增 XmmMaskedStore 及原生 RAM 按字节选择写入，覆盖两个地址宽度形式，CpuSimdHIR
  合计 450。初始 DI 按地址宽度截断，后续选中字节继续线性递增。
- 保留基线的完整 16 字节可写检查，包括全零掩码。慢路径在检查完成后一次性采样源与
  掩码；设备写回调无法改变本次捕获输入，CPU 回调状态及部分故障保持到退出。
- 1,792 个夹具与全部 65,536 种掩码通过 debug/release CPU 和独立标量参考比较；已验证
  页表回调在检查期间修改源/掩码/EDI、后续缺页、零/稀疏掩码权限、MMIO、脏 XMM
  及终端存储后续执行。见 [masked-store 契约](ir-simd-masked.md)。
- 生产 Pending 仍为 3,728；完整 IR-00～IR-14 验收条件尚未满足。
- 最终全套回归在单次 make 调用中通过：87 个 warnings-as-errors Rust 测试及全部已接入
  IR/独立参考套件，包含 24 个精确 MMIO 写事件检查。日志为
  `build/ir-simd-masked-full-suite.log`；feature 编译见 `build/ir-simd-masked-check.log`。
  目录归属、普通构建导出隔离及 diff 检查通过。


## 后续推进：x87 寄存器形式

- 新增 D8-DF `mod=3` 的 terminal CpuX87Helper；成功路径直接复用解释器的 F80 指令体，不将 x87 降成 host f64，也不调用 legacy JIT emitter。
- CR0.EM/TS 先通过 x87 task-switch guard 交付 #NM；随后同步并清空 legacy x87 f64 shadow cache，再进入 canonical F80 状态。nested 非法子编码在调用语义体前显式判定，#UD 不会误提交指令。
- experimental catalogue 新增 160 个 CpuX87Helper coarse forms，experimental Pending 从 1110 降至 950；生产 Pending 仍为 3,728。x87 memory/environment、MMX 和剩余 FP/SIMD 工作继续属于 IR-08。
- `make ir-x87-tests` 作为 IR-core 门禁，使用全部 D8-DF register subencoding、D9/DD operand-size 变体、F80/TOP/tag/status、FCMOV/FCOMI flags、nested #UD 与 EM/TS #NM priority 做 interpreter differential。契约见 [ir-x87.md](ir-x87.md)。

## 后续推进：显式 MIR 访存计划

- Lowering 为标量访存、部分存储、RMW 读取和全部已接入 XMM 访存生成 MemoryPlan。
  TLB 权限/访问范围、原生物理动作、慢路径完整 ABI 和恢复/退出策略由计划承载；
  发射器直接消费计划，移除了这些路径中按 HIR opcode 重新选择访存语义的分支。
- 已加入发射前 HIR/计划一致性检查、访存与通用 helper 的导入签名冲突检查及 MIR dump。
  17 类访存形式、ENTER 部分完成、10 类不安全修改、过期计划和导入冲突的测试通过。
- MIR 仍共享 HIR 值/状态/CFG 与原有分配器。独立 MIR CFG、RMW 提交、CMPXCHG8B、
  地址/检查/其他动作及动态预算账本仍待迁移；本次不宣称完整 IR-03/09 已完成。
  详见 [MIR 访存迁移契约](ir-mir-memory.md)。
- 全套回归在单次 make 调用中通过：90 个 warnings-as-errors Rust 测试及全部已接入
  IR/独立参考套件，日志为 `build/ir-mir-memory-full-suite.log`。feature 编译见
  `build/ir-mir-memory-check.log`；目录、普通导出隔离及 diff 检查通过。ISA 覆盖未变。


## 后续推进：MIR 观察点、RMW 提交与守卫调用

- 新增 EffectPlan，承载 SegmentAddress、PopAddress、GuestCheck、SseCheck 和 RmwStore。
  发射器直接消费计划；段全局地址、调用 ABI、守卫条件、成功/异常策略均由 lowering 决定。
- RMW Observation 显式区分提交后的寄存器/FLAGS 与写入前计数，保留设备观察顺序和最终
  单次计数。MIR dump 已包含完整 HIR 定义与状态图，便于追踪计划中的值及恢复点。
- 已加入 effect/通用 helper 签名冲突检查；集中保护全部 22 个内建 CPU 导入名，补齐新增
  XMM adapter 的防覆盖检查。三组 warnings-as-errors 测试已通过，覆盖 11 类错误计划、
  缺失计划、RMW 三种宽度、地址/守卫语义及导入冲突。
- 独立 MIR CFG、CMPXCHG8B/除法/其余动作、动态计数/预算以及在线 Tier 集成仍未完成。
  详见 [MIR effect 契约](ir-mir-effects.md)。ISA 覆盖与生产 Pending 未变。
- 全套回归在单次 make 调用中通过：93 个 warnings-as-errors Rust 测试及全部已接入
  IR/独立参考套件，日志为 `build/ir-mir-effect-full-suite.log`。feature 编译见
  `build/ir-mir-effect-check.log`；目录、普通构建导出隔离和 diff 检查通过。


## 后续推进：MIR 除法与 CMPXCHG8B 计划

- 新增 checked-division 与 compare/exchange 计划，统一纳入 EffectPlan 验证、导入签名
  检查和 dump。发射器不再从 HIR opcode 选择这两类指令的范围、隐含寄存器和异常 ABI。
- 除法计划显式记录商范围与 i64 MIN/-1 保护，保留最后一个异常检查后再分配两项结果的
  顺序；CMPXCHG8B 计划明确八字节写 guard、比较/替换槽位、ZF backing 更新和单次计数。
- 24 个除法计划组合、16 个 CMPXCHG8B 计划组合及 16 类不安全修改测试通过，按
  warnings-as-errors 编译。见 [MIR arithmetic 契约](ir-mir-arithmetic.md)。
- 独立 MIR CFG、其余动作、通用 helper 观察策略、显式状态物化和动态预算/计数，以及
  在线 Tier 集成仍未完成；ISA 覆盖及生产 Pending 未变。
- 全套回归在单次 make 调用中通过：96 个 warnings-as-errors Rust 测试及全部已接入
  IR/独立参考套件，日志为 `build/ir-mir-arithmetic-full-suite.log`。feature 编译见
  `build/ir-mir-arithmetic-check.log`；目录、普通导出隔离和 diff 检查通过。


## 后续推进：MIR 通用 helper 调用点计划

- 新增 CallPlan，明确 CPU/standalone 观察策略、参数、逆序 typed 结果暂存、调用者异常
  恢复/派发、可接受退出和正常续执行。发射器直接消费计划，保留结果检查后再赋值的顺序。
- 共享 helper 表的 ABI 合法化已集中；发射前同时验证表与调用点，拒绝改变签名、结果顺序、
  状态观察和 outcome 策略的修改。活跃调用必须有计划，已移除的 arena 记录不要求活跃导入。
- 三组 warnings-as-errors 测试通过，覆盖异常所有权、两种优化设置、CPU/standalone
  观察、多结果、CPUID/REP 终端、13 类错误修改和 arena 残留。见 [调用点契约](ir-mir-calls.md)。
- 独立 MIR CFG、显式状态物化、动态预算/计数、完整 ISA 和在线 Tier 集成仍待完成；
  生产 Pending 保持 3,728，完整 IR-00～IR-14 尚未达到验收条件。
- 全套回归在单次 make 调用中通过：99 个 warnings-as-errors Rust 测试及全部已接入
  IR/独立参考套件，日志为 `build/ir-mir-call-full-suite.log`。feature 编译见
  `build/ir-mir-call-check.log`；目录、普通导出隔离和 diff 检查通过。


## 后续推进：MIR dispatcher CFG 与 typed 边复制

- 新增 ControlFlow，承载入口、块、指令顺序、预算恢复点、终结指令和 local 边复制计划。
  Wasm 发射器已改为消费 MIR 图，不再遍历 HIR blocks/entries 或自行计算 HIR CFG。
- 平行复制在 lowering 排序：去除自复制，无环路径直接写入，复制环只暂存一个源；重复源
  随同重定向，保留临界边仅在选中分支执行和恢复点观察完整参数的语义。
- 独立符号模型穷举三种 local 类型的 150,207 组源映射；648 次 Wasm 执行验证全部三源
  映射、优化开关、两分支与预算恢复。另有多入口循环和 12 类不安全图/复制修改检查。
  详见 [MIR 控制流契约](ir-mir-control.md)。
- 当前图仍保持 HIR 拓扑，指令/状态 arena 与分配器继续共享；独立 MIR 图变换和验证、
  显式物化、动态 CPU 计数及在线 Tier 尚未完成，完整 IR-00～IR-14 验收条件仍未满足。
- 全套回归在单次 make 调用中通过：102 个 warnings-as-errors Rust 测试及全部已接入
  IR/独立参考套件，包含实际 i32/i64/v128 边复制和最终 FLAGS 观察检查。日志为
  `build/ir-mir-control-full-suite.log`；feature 编译见 `build/ir-mir-control-check.log`。
  目录、普通导出隔离与 diff 检查通过。


## 后续推进：MIR 值程序与 packed kernel 选择

- 新增 ValuePlan，明确标量机器宽度、窄有符号输入扩展、结果掩码、转换、位插入/提取、
  读取 ABI 和 SIMD lane 操作。Wasm 指令分派已完全消费计划，不再匹配 HIR opcode。
- 新增 PackedPlan，共用于寄存器计算和原生向量访存组合，明确 unpack、超界移位、
  AND-NOT 输入顺序、高半乘法和 SAD 的 opcode/重排策略；CPU 慢路径契约保持一致。
- 计划一致性检查之外，新增仅依赖机器操作与值类型的栈类型检查。三组聚焦测试已通过，
  覆盖 60 个标量宽度/操作组合、228 个 packed 选择组合及错误栈/不安全计划。
  独立 BigInt 模型已通过 17,280 次 Wasm 执行。见 [值程序契约](ir-mir-values.md)。
- HIR 仍提供值/状态 arena 与 liveness；独立 MIR 生命周期、显式物化、动态 CPU 计数、
  完整 ISA 和在线 Tier 接入仍未完成，生产 Pending 保持 3,728。
- 全套回归在单次 make 调用中通过：105 个 warnings-as-errors Rust 测试、17,280 次新增
  标量执行及全部已接入 IR/独立参考套件，包含全部 SIMD 家族与最终 FLAGS 观察检查。
  日志为 `build/ir-mir-value-full-suite.log`；feature 编译见 `build/ir-mir-value-check.log`。
  目录、普通生产导出隔离和 diff 检查通过。


## 后续推进：MIR 有序状态物化计划

- 新增 StatePlan，明确 CPU/standalone 的 GPR、XMM、FLAGS backing、last_op1、恢复 PC
  写回及计数阶段。发射器在出口、预算、访存/helper 观察与故障恢复时直接执行计划，
  不再解释 HIR StateMap 字段。
- RMW 中间观察明确选取提交后的值计划和写入前计数计划，不再临时复制/修改 StateMap。
  decoded-next 观察写回与故障 previous_ip 分开，保留 CS 加法和计数回绕语义。
- 三组 warnings-as-errors 聚焦测试通过；48 个模块完成 3,456 次状态观察执行，覆盖
  四种 PC 模式、raw/lazy ZF、GPR/XMM、优化与重新进入。12 类错误计划及过期状态被拒绝，
  RMW 三宽度分阶段观察检查通过。见 [状态物化契约](ir-mir-state.md)。
- 计数仍基于静态快照；动态 CPU 循环账本、独立 MIR 生命周期、完整 ISA 和在线 Tier
  接入仍未完成，完整 IR-00～IR-14 验收条件尚未满足。
- 全套回归在单次 make 调用中通过：108 个 warnings-as-errors Rust 测试、3,456 次新增
  状态观察执行及全部已接入 IR/独立参考套件，包含实际 CPU 故障/回调、RMW、向量出口和
  最终 FLAGS 观察检查。日志为 `build/ir-mir-state-full-suite.log`；feature 编译见
  `build/ir-mir-state-check.log`。目录、普通生产导出隔离和 diff 检查通过。


## 后续推进：SSA 动态计数与可执行 CPU 循环

- StateMap 新增 count_base，实际计数为 SSA 基数加静态指令内偏移；纳入支配、活跃性、
  local 分配和优化重写。提交前后必须共用基数，保留既有的偏移/PC 提交约束。
- MIR 明确基数与偏移，CPU delta 账本记录运行时总和；全部活跃观察/恢复点具备动态基数
  的循环现可发射 CPU Wasm，静态或混合计数循环继续拒绝。孤立 state arena 记录不阻碍准入。
- 三组聚焦测试通过；独立模型通过 7,786 次循环执行、17,604 次 helper 状态观察及 106 次
  预算后重新进入，包含故障单次派发、helper 接管状态、计数回绕及不重复提交。
  详见 [动态计数契约](ir-dynamic-count.md)。
- 当前字节前端仍使用静态计数；自动 guest CFG lifting、在线调度/中断重入、完整 ISA、
  XP/性能与旧路径退役仍待完成，完整 IR-00～IR-14 验收尚未达到。
- 全套回归在单次 make 调用中通过：111 个 warnings-as-errors Rust 测试、新增动态 CPU
  循环执行及全部已接入 IR/独立参考套件，包含全部 SIMD 家族与最终 FLAGS 观察。
  日志为 `build/ir-dynamic-count-full-suite.log`；feature 编译见 `build/ir-dynamic-count-check.log`。
  目录、普通生产导出隔离及 diff 检查通过。

## 后续推进：从实际 x86 字节构建 CFG

- 新增 `lift_cpu_cfg` / `compile_cpu_cfg_region`，沿可达直接分支和 fallthrough
  构建带 GPR/FLAGS/XMM/effect/动态计数参数的 HIR；回边不会重新读取入口寄存器。
- 区域外和间接目标保留出口；store、RMW 与 CPU 接管 helper 保留终端边界。
  跳过不可达字节，拒绝重叠指令流、可达解码失败和预算溢出。
- 35,328 次真实 CPU 差分、173,056 个解释器步骤和 16 次第二轮真实 #PF 对照通过，
  包括优化前后、16/32 位、CS/EIP 回绕、预算、FLAGS/XMM 和退休计数。
- 当前为单入口、逐指令 block 的冷编译 API；在线区域选择、发布、调度和完整 ISA
  仍未完成。详见 [CFG 前端契约](ir-cfg-frontend.md)。
- 本轮完整关联矩阵通过：`build/ir-cfg-full-suite.log`，包含 113 项 warnings-as-errors
  Rust 测试、全部现有 IR/独立参考目标及最终 FLAGS 观察测试；实验 Wasm 编译、目录、
  正式构建导出隔离与空白检查通过。没有运行 XP 或在线 IR 工作负载。

## 后续推进：直线块合并与显式预算检查

- 默认优化新增有界直线 block 合并：消除单前驱、非入口直接后继的 dispatcher 边，
  重写 SSA/effect/StateMap 并压缩 block arena；保留多前驱合流及终端 CPU 边界。
- `PollBudget` 已接入 MIR/Wasm，在被合并的原 block 入口保留相同恢复映射和预算成本。
  GVN/DCE 不能删除该检查或其恢复值；可以通过 `PassConfig.merge` 单独关闭合并。
- 35,328 次 CPU 差分、17,664 次精确预算出口对照及 16 次第二轮 #PF 对照通过。
  详见 [合并与预算契约](ir-cfg-merge.md)。在线调度及完整 IR-00–IR-14 仍未完成。
- 完整关联矩阵通过：`build/ir-merge-full-suite.log`，包含 115 项 warnings-as-errors
  Rust 测试、全部 IR/独立参考目标、SIMD 各族及最终 FLAGS 观察测试；实验特性编译、
  目录、正式导出隔离和空白检查通过。没有验证在线调度、XP 或实际工作负载性能。

## 后续推进：跨块纯值复用与常量分支裁剪

- GVN 按支配关系复用纯表达式，区分 SSA 操作数和机器类型；排除 CPU 状态读取、
  访存、helper 与观察点。兄弟分支、独立入口的值不能相互替代。
- 新增可独立关闭的常量/同边条件分支裁剪；从全部入口计算可达性，并压缩重写
  block/instruction/value/StateMap arenas。后续合并继续保留存活路径的预算检查。
- 3,072 次独立 Wasm 菱形 CFG 对照、39,936 次 CPU 差分、19,968 次精确预算对照、
  16 次第二轮 #PF 和 16 次跳过缺页读取的常量分支对照通过。
  详见 [数据流优化契约](ir-dataflow.md)。完整 IR-10 及后续验收仍未完成。
- 完整关联矩阵通过：`build/ir-dataflow-full-suite.log`，包含 118 项 warnings-as-errors
  Rust 测试、全部 IR/独立参考目标、SIMD 各族和最终 FLAGS 观察测试；实验特性编译、
  目录、正式导出隔离与空白检查通过。没有运行在线 IR、XP 或实际工作负载性能测试。

## 后续推进：实际异步发布桥接的任务校验与失败回收

- Rust/JS 在线桥接新增不回绕的 u64 任务票据及安装前校验；旧回调不能覆盖或清除
  复用槽位。JS 固定 Wasm 字节快照，并验证 Wasm 实例/exports/table 身份。
- 缓存清空与快照恢复立即取消在途预留；浏览器编译、函数导出和 table 安装失败会
  回收对应槽位。最多记录 128 个失败任务，对未变代码抑制重复编译，依赖页写入可重试。
- debug/release 各通过 159 次受控实例化；899 槽位压力、旧回调、SMC、恢复及记录器
  票据透传测试通过。详见 [发布契约](ir-publication.md)。
- 同步浏览器异常延后到微任务回收，避免重入生成器持有的 JIT 锁；替换编译失败后，
  通过实际 JIT 执行计数确认原模块保留并继续执行。
- 这修正的是生产仍在使用的共享 legacy 发布桥接；IR Artifact 尚未接入该缓存，
  没有据此宣称 IR-12、在线 IR 或完整目标已经完成。
- `make all`、生产 CPU/FLAGS/层级/容量回归及 Worker CPU、音频、UI 测试通过。
  Worker GPU 像素测试间歇失败，固定基线也复现同样现象；API 保存/恢复首次卡住，
  后续两次复测通过，其余已运行 API 通过，PIC 因缺少 fixture 跳过。没有将这些
  不完整结果记作完整浏览器/API 验收，详见测试报告中的原始日志。
- 完整 IR 关联矩阵通过：`build/ir-publication-full-suite.log`，包含 118 项
  warnings-as-errors Rust 测试、全部现有 IR/独立参考目标、SIMD 各族及最终 FLAGS
  观察。实验特性编译见 `build/ir-publication-check.log`；目录、正式导出隔离及
  空白检查通过。生产 Pending 仍为 3,728，没有据此宣称在线 IR 或性能验收通过。

## 后续推进：CPU 编译入口的特化键与无副作用拒绝

- 两个 CPU CompileRequest API 产物新增入口契约，记录 guest EIP、线性地址和默认
  解码宽度；发布检查同时核对任务键、物理依赖版本和入口契约，区分 standalone/CPU。
- 生成 Wasm 在 `ir_enter` 之前校验入口编号、IP/CS/宽度以及非 legacy、非前缀中途、
  非 HLT 状态；不匹配时不修改客户机状态、REP 结果，不触发访存或语义 helper。
- debug/release 各通过 1,760 次无副作用拒绝、176 次 CPU 解释器对照及 32 次真实
  页故障对照；涵盖物理页别名、CS 回绕、模式、线性/CFG API、优化与 SIMD。
  详见 [CPU 入口契约](ir-entry.md)。
- 启用 IR 的 release 构建现在也维护实际 legacy 帧标记；真实 JIT I/O 回调在记录器
  开/关两种模式下均拒绝冷 IR 入口，返回后标记恢复。该验证不依赖测试钩子伪造标记。
- 该校验不验证代码版本或槽位生命周期；底层 fixture 发射 API 仍由调用方建立上下文。
  IR Artifact 的在线缓存/调度接入、其余 ISA 与完整 IR-00～IR-14 仍未完成。
- 完整 IR 关联矩阵通过：`build/ir-entry-full-suite.log`，含 120 项 warnings-as-errors
  Rust 测试、全部现有 IR/独立参考目标和最终 FLAGS 观察。最终构建的真实帧校验及
  CPU 回归另见 `build/ir-entry-final-focused.log`、`build/ir-entry-cpu-regression.log`。
  实验特性编译、无测试钩子的实验 release、普通生产导出隔离及空白检查通过。

## 后续推进：CPU Wasm 内部编译与只读代码快照

- 新增只读 RAM 页表遍历，保留已有 TLB 的可见映射，覆盖普通分页、PAE/PSE、
  cached PDPTE、用户读权限及地址回绕；拒绝 MMIO/缺页，不修改页表 A/D、TLB 或客户机状态。
- 快照显式记录线性页到物理页的映射，物理依赖去重；允许跨页物理别名，并在编译和
  产物校验中拒绝缺失、错序、未对齐或依赖不一致的映射。
- `ir_compile_live` 已在实验 CPU Wasm 内实际执行完整现有 IR 编译链，提供受句柄保护的
  字节/元数据读取、释放及重新校验。缓存清空/恢复和已通知的依赖写入会使结果失效，
  另以精确字节/映射重读覆盖未经过旧代码页通知的写入。
- debug/release 各通过 44 个实时编译执行对照、8 个真实数据页故障、19 个分页/快照
  用例及生命周期检查；无测试钩子的实验 release 也通过 CFG/store 实际编译执行。
  详见 [运行中 CPU 编译契约](ir-live-compile.md)。
- 当前是单个未发布产物，未预留或安装 table 槽位；依赖版本仅属于该任务，不能据此
  宣称完成全局失效图。在线 IR 缓存/调度、剩余 ISA、系统/性能验收和旧路径退役仍待实现。
- 最终代码通过完整 IR 矩阵（121 项 Rust 测试和全部既有差分目标）、实验特性编译、
  无测试钩子的实验 release CPU 回归；日志为 `build/ir-live-full-suite.log`、
  `build/ir-live-check.log`、`build/ir-live-cpu-regression.log`。
- 普通 debug/release 构建和实验构建导出隔离、覆盖目录及空白检查通过；生产
  Pending 仍为 3,728，本轮未新增浏览器宿主、OS 启动或性能验收结果。

## 后续推进：IR 产物发布与正常 CPU 分派

- 新增内部 `CPU.ir_compile_cached` 异步发布流程，将实时编译产物转入既有 899 槽池；
  最多保留 32 个 IR 记录，预留槽纳入 legacy 容量不变量，安装前检查完整句柄和槽位。
- Published IR 入口已参与 `cycle_internal` 分派，支持入口键校验、计数及性能记录；
  正常取指翻译保留 A 位更新，并重校验代码/页表别名；冷的后续代码页不会被提前访问。
- 注册全部物理依赖页及已有 TLB 别名的写入监视；写入、取消和恢复立即停止未来命中，
  活跃 I/O 回调中的失效推迟到函数返回后才回收槽位，零退休 REP 出口可继续解释执行。
- debug/release（含槽池不变量）和无测试钩子的实验 release 通过实际缓存分派、精确 #PF、
  A 位/跨页/代码页表别名、SMC、异步失败及迟到完成测试；两种不变量构建各完成
  900 次 legacy 发布/淘汰，保留 32 个 IR 入口且仍能执行。
  日志：`build/ir-cache-final-focused.log`，契约见 [IR 缓存](ir-cache.md)。
- 实验 release 的 CPU 回归通过：`build/ir-cache-cpu-regression.log`。自动区域发现、
  IR 编译触发/升档、完整共享版本和链接图、其余 ISA、OS/性能及旧路径退役仍未完成。
- 完整 IR 矩阵通过：`build/ir-cache-full-suite.log`，含 121 项 Rust 测试及全部既有
  差分目标。最终新增的 JIT 锁可用性守卫由 `build/ir-cache-final-focused.log` 单独验证，
  拒绝在 legacy 生成器持锁的同步宿主回调内发布或回收 IR 槽位。
- 最终入口/冷编译复测、`make all`、普通 899 槽压力和 debug/release 各 159 项
  legacy 事务发布用例通过：`build/ir-cache-final-regression.log`。生产 Pending 仍为 3,728。
- 实验特性检查、普通 debug 构建、生产/实验导出隔离和空白检查通过；本轮没有新增
  浏览器宿主、OS 启动或性能验收结果，IR-00～IR-14 的原始验收范围仍保持不变。

## 后续推进：自动 IR 编译、链接热度与优化升档

- 新增默认关闭的 `ir_auto_config` 策略；普通入口和关闭 recording 时的 legacy 链接
  入口累计热度，编译/发布仅发生于外层冷 CPU 调度点，每个主循环至多检查一个候选。
- Tier 1 选择最多 32 条指令并使用轻量管线；Tier 2 选择最多 48 条、较大字节窗口，
  执行现有优化 pass。升档发布成功前保留原入口，产物显式记录 tier 元数据。
- 自动任务共享不回绕的实例身份、槽池和事务发布桥；最多跟踪一个当前 pending 任务，
  128 条热度/失败记录和 32 个缓存记录；按代码/映射抑制重复失败，写入和恢复允许重编译。
- 自动淘汰只选择自动发布的入口，并重置被淘汰入口的历史热度；保留显式入口和升级目标，
  避免工作集大于缓存时不断重编译不再执行的旧入口。
- debug/release 槽池不变量构建及无测试钩子的实验 release 通过自动 Tier 1/2 执行、
  16 位 CS/AX、42 指令循环的精确计数、失败升档保留 Tier 1、pending/恢复/迟到完成、
  40 入口淘汰及既有显式缓存矩阵；两种不变量构建另验证真实 legacy 链接热度和冷发布。
  日志：`build/ir-auto-final-focused.log`；契约见 [自动 IR 策略](ir-auto.md)。
- 新增 `env.ir_codegen_finalize`，JS 与实验 Wasm 需一起重建；zstd worker 也已补齐
  导入以维持快照流程。尚未增加 starter/Worker 策略选项，完整 ISA、独立 MIR、共享
  链接/版本图、OS/性能验收和生产旧路径退役仍未完成。
- 完整 IR 矩阵通过：`build/ir-auto-full-suite.log`，含 121 项 Rust 测试和全部既有
  差分目标。最终的“无候选时也限制每主循环一次扫描”改动由最终自动/缓存复测单独覆盖。
  覆盖目录检查通过，生产 Pending 仍为 3,728。
- 最终自动策略复测还关闭了 legacy 编译：三种实验构建均能完成 IR 升档和精确循环
  执行，legacy 发布器调用为零（`build/ir-auto-policy-final.log`）。
- 最终入口/冷编译、`make all` 和普通 899 槽压力通过。legacy 发布测试的固定 5 毫秒
  等待在并发压力下先于客户机执行结束；已改为等待 HLT，再保留原结果与 JIT 步数校验。
  debug/release 各 159 项测试及 8 次并发复测通过，日志见
  `build/ir-auto-publication-fixed.log`、`build/ir-auto-publication-fixed-stress.log`。
- 实验特性检查、普通 debug 构建、生产/实验导入与导出隔离、空白检查通过。
  本轮未新增浏览器宿主、OS 启动或性能验收，完整 IR-00～IR-14 目标仍未完成。

## 后续推进：有界 Tier 2 优化与 RAM 读取复用

- 纯 SSA 循环不变量外提已接入优化 Tier 2；内存、CPU 观察、异常和预算恢复点不移动。
- 独立 MIR 可为同块、同地址、同宽度的普通读取生成有界静态复用证书；发射器仅在原生
  RAM 守卫成功后置位运行时有效位，进入慢路径前清零。MMIO 回调、重映射和缺页保留原路径。
- 成功继续的一单位预算检查可保留复用；检查本身及其物化后返回的出口不删除、不移动。
  不支持把证书跨块/入口使用，也不对并发共享内存作保证。
- 已加入伪造证书拒绝、预算失败原子性、Tier/优化开关与真实 CPU 对照测试；入口为
  `make ir-forwarding-tests`。另有确认实际发生 LICM 后再进入循环内缺页的 CFG 回归。
- 这些改动不新增 ISA 形式，不改变默认后端、快照 ABI、发布/失效规则或 legacy 退役门槛。
  完整 IR-00～IR-14、XP、应用和性能验收仍未完成。详见
  [循环优化说明](ir-licm.md)及 [RAM 读取复用说明](ir-ram-forwarding.md)。

## 后续推进：受守卫的 store-to-load forwarding

- 在既有 owned MIR RAM forwarding 证书上增加已提交标量 store 作为链源；仅接受 8/16/32 位、同地址、同宽度、规范普通 RAM ABI，store 本身绝不删除。
- 只有 native same-page writable RAM 写入实际完成后才把写值放入 forwarding cache；随后仍执行现有物理代码依赖页别名检查，命中当前代码页时立即退出，不允许复用陈旧代码后的执行。
- store/load 任一慢路径在页表遍历、MMIO 或 callback 前清空有效位；成功的慢 store 继续按既有契约退出当前 IR 入口，因此不会把 callback 结果错误升级为 RAM 证明。
- 新增 8/16/32 位证书回归、宽度/地址/段屏障，以及 CPU Wasm 的 store→load 执行对照；现有慢 store 退出、物理代码页别名退出和后续 #PF 精确恢复测试继续共用同一套件。
- 该增量推进 IR-11，但不改变生产默认 backend、ISA 覆盖门槛、共享版本图或 IR-14 legacy emitter 退役条件。


## 后续推进：保守 CPU state-write elision

- lowering 在 HIR/CFG edge 仍可见时，对 GPR、XMM 及 FLAGS provenance 来源做固定点传播，生成每个 StateMap 的 CPU write 省略证书；完整入口 lazy-FLAGS backing 现已纳入证明，包括 raw EFLAGS、flags_changed、last_result、last_op1 与 last_op_size。
- 当前只对单外部入口、没有可继续 memory/helper/commit 观察点的 CPU CFG 生效；PollBudget 与 guarded SseCheck 的观察分支允许，因为一旦物化就立即退出当前 IR entry。遇到可继续慢路径则整区使用全 false 证书，不猜测 CPU backing 历史。
- 当六个 arithmetic flags、system flags 以及全部 lazy backing 都仍严格等于入口时，EFLAGS、last_result、last_op_size、flags_changed 与 last_op1 可整体保留原 CPU backing；任一来源变化则继续走原 canonical materialization。EIP、previous_ip 与退休计数始终写回。Tier 1、standalone、关闭优化和 zero-round 配置不启用。
- optimized Tier 2 记录 passes.state_writes_elided；现有 CFG CPU differential 同时覆盖优化前后精确预算出口、FLAGS/XMM、previous_ip、退休计数及既有 fault case。
- 该增量为后续跨块 FLAGS demand/liveness 和 resumable observer dirty-state 合流建立基础，不改变 ISA coverage、默认 backend 或 IR-14 退役门槛。详见 [state-write elision](ir-state-elision.md)。


## 后续推进：完整 lazy-FLAGS backing provenance

- FlagState/StateMap 新增 raw EFLAGS、完整 flags_changed、last_result 与 last_op_size 的入口 provenance，并与既有 last_op1、raw ZF、zero-is-lazy 及六个 arithmetic flags 一起穿过 CFG block parameters、edge 参数和优化重写。
- 新增 ReadFlagResult / ReadFlagSize HIR 入口读取及 MIR value lowering；verifier 要求完整 backing 要么全部存在、要么全部缺失，并继续限制所有 CPU backing 读取只能位于 external entry。
- state-write elision 只有在 semantic FLAGS 与全部 lazy backing 均严格 entry-equivalent 时才省略 EFLAGS/lazy backing 写回；若任何 flag 被修改，则继续使用既有 canonical FLAGS materialization，不改变异常恢复语义。
- CFG CPU differential 对纯自循环新增 bit-for-bit backing 校验，覆盖 flags、flags_changed、last_result、last_op1 与 last_op_size；完整 IR-core 同时保留原 FLAGS、fault、SIMD、RAM forwarding 和 store-continuation 回归。
- 该增量为下一步 partial FLAGS demand/liveness 建立完整恢复证明，但尚未删除仍被 recovery StateMap 需要的 changed flag SSA，也不改变默认 backend、生产 coverage 或 IR-14 门槛。


## 后续推进：CPU-only partial FLAGS liveness

- 在完整 lazy-FLAGS provenance 上新增 backing_valid SSA 证明位；首批精确建模 ADD、SUB/CMP、AND/OR/XOR 及复用这些语义的 TEST/NEG/XADD/CMPXCHG。ADC/SBB、INC/DEC、shift/rotate、bit、multiply、SAHF/BCD 等未完整建模的混合布局显式失效，后续不自动恢复 validity。
- CPU StatePlan 在 validity 可静态证明为 true 时直接恢复 raw EFLAGS、flags_changed、last_result、last_op1、last_op_size；standalone 仍使用六个 concrete arithmetic flags 的原计划，通用 HIR/StateMap 不删字段。
- lowering 生成有界 CPU-only SSA liveness 证书，从 CPU StatePlan、显式条件分支、ordered/effect/helper/poll 反向追踪；对 block parameter 仅沿各 predecessor 的对应 edge arg 回溯，并用 visited value 集保证循环固定点有界。
- Tier 2 CPU emitter 仅跳过证书判定 dead 的纯 value program；访存、effect、helper、预算检查及 Tier 1 不受影响。新增 passes.cpu_values_elided 作为静态省略计数。
- focused 回归要求 ADD→ADD→JNZ 的 CPU Wasm 变小而 standalone byte-identical；INC 保持 canonical recovery。真实 CFG 差分新增 ADD/SUB/AND/CMP/JNZ，对同一预算出口逐字段比较 raw flags、flags_changed、last_result、last_op1、last_op_size 与解释器。
- 本增量仍不是完整 FLAGS liveness：需继续覆盖 ADC/SBB、INC/DEC、shift/rotate 等 backing 形式，并处理 resumable helper/MMU callback 后的 dirty-state 合流。详见 [partial FLAGS liveness](ir-flags-liveness.md)。


## 后续推进：ADC/SBB 与 INC/DEC mixed FLAGS backing

- ADC/SBB 现按 v86 基线保留 eager CF/AF/OF：先在 raw EFLAGS 中清除并写入三个位，再把 flags_changed 设为仅 PF/ZF/SF lazy；SBB 同时保留 FLAG_SUB。last_op1、last_result、last_op_size 与结果宽度继续精确记录。
- INC/DEC 现把输入的 architectural CF 写回 raw EFLAGS，并让 AF/ZF/SF/OF/PF 保持 lazy；DEC 的 flags_changed 带 FLAG_SUB。该实现复用既有 concrete CF SSA，因此即使输入 CF 原本是 lazy 也不会误用旧 raw bit。
- validity 不会从 false 自动恢复；只有从已知精确 backing 进入这些指令时才继续保持精确证明，先前发生未建模 FLAGS 修改后仍走 canonical materialization。
- 真实 CFG differential 新增 ADC→SBB→INC→DEC→JNZ，覆盖 16/32 位默认模式，并增加 8 位 ADC/SBB/INC/DEC 版本；optimized raw flags、flags_changed、last_result、last_op1、last_op_size 与解释器在同一预算出口逐字段一致。
- shift/rotate、bit、multiply、SAHF/BCD 等 mixed eager/lazy backing 仍待后续逐类审计；默认 backend、生产 coverage 与 IR-14 条件不变。


## 后续推进：FLAGS backing 扩展批次

- SHL/SHR/SAR、SHLD/SHRD 按有效 count 建模：count=0 完整保留输入 backing；非零时 raw EFLAGS 仅 eager 更新 CF/OF，flags_changed 保留 PF/AF/ZF/SF lazy，并精确更新 last_result 与 last_op_size。
- ROL/ROR/RCL/RCR 仅更新 eager CF/OF 并从原 lazy mask 清除两位，不改变 last_result/last_op1/last_op_size；through-carry 继续使用 concrete architectural CF SSA。
- BT/BTS/BTR/BTC 仅 eager 更新 CF；BSF/BSR eager 固定 CF/ZF 并保留基线 undefined FLAGS 的 lazy 规则；POPCNT 将全部 arithmetic flags 转为 eager。
- MUL/IMUL 精确恢复 eager CF/OF、last_result/op-size，并保留基线对其余 undefined arithmetic flags 的 lazy 表示。
- CLC/STC/CMC 只同步 raw CF 与 lazy mask；CLD/STD 同步 raw EFLAGS 的 DF bit 与 system flags，不再整体失效 backing。
- shifts/bits/multiply 专项 differential 增加 raw EFLAGS、flags_changed、last_result、last_op1、last_op_size 与解释器逐字段比较；CFG 另覆盖 carry/direction 多 budget 边界。


## IR-10 完成说明

- 已将 scalar alias 与 constant rewrite 拆分，新增独立 bounded copy pass、`PassConfig.copy` 与 `PassStats.copied`；StateMap、branch condition、CFG edge args 与 instruction args 原子重写。
- `PassConfig.flags` 独立控制 Tier 2 CPU-only FLAGS/value liveness；exact lazy backing 不需要 concrete flag 时可由 DCE/CPU emitter 省略，特殊 backing 自动退回 canonical recovery。
- 新增 helper-state lowering certificate：仅对 `Effects::pure + CannotFault + normal_preserves_state` 且无 fault delivery/exit 的 nonterminal helper 省略 pre-call CPU StateMap observation；state-reading/faulting/terminal helper 为硬屏障。
- CPU liveness 同时保存 conservative baseline mask 与 helper-trimmed mask；只有先启用 helper-state 后才选择 trimmed roots，关闭任一开关均回到保守行为。
- 新增 `tests/ir/semantics/ir10.rs` 和 `tests/ir/differential/ir10.mjs`，对 copy、DCE、GVN、CFG、FLAGS、helper-state 分别单独启用并做 baseline/optimized Wasm 执行对照；该 differential 已加入 IR-core merge gate。
- 完成边界见 [IR-10 completion](ir10-completion.md)。RAM/TLB proof、forwarding、LICM、强度削弱及高级 SIMD/循环优化继续属于 IR-11；默认 backend、生产 coverage 与 IR-14 门槛没有改变。


## 后续推进：五项覆盖与后端实现

- 新增远 CALL/JMP/RETF、INT/INTO/IRET 的终端 CPU ABI，以显式 completion 区分成功提交和已派发故障；保留现有 CPU 模型及其限制。
- D8–DF memory/environment 已接入 canonical F80；FXSAVE/FXRSTOR 与 MXCSR 已接入终端状态适配。#NM/#UD、段检查、分页、MMIO 和 stack-pop 顺序有独立差分。
- 补齐 F6/F7 /1 TEST alias，以及 LES/LDS/LSS/LFS/LGS 非法寄存器形式的显式 #UD。
- 新增 STI shadow 与嵌套 STI 片段、MMX、SSE FP/conversion，以及 ARPL/FWAIT/RDRAND/MOVNTI 和显式基线保留形式。
- byte frontend 的 CPU CallHelper 增加集中签名/outcome/exception-owner registry；CpuReload 在正常返回后重载 GPR/FLAGS/XMM，并支持 CFG 续执行和动态 lazy-FLAGS 保留。
- MIR 增加受预算约束的纯值栈融合与 HIR 丢弃后的 typed local 分配，保留恢复根并重建 parallel edge copies；预算失败不发布部分结果。低层 memory import 仍保持独立类型契约。
- `make ir-control-reference-tests` 使用 debug/release 和固定旧语义版本进行对照，并接入 IR-core。契约、基线限制及测试范围见 [本批说明](ir-control-fp-increment.md)。
- Experimental Pending 950 → 0；production Pending 保持 3,728。粗粒度清零包含原基线不支持、debug assertion/release #UD 等行为，不能代替完整前缀/模式/异常验收；XP、应用、性能、无 SIMD 降级与 IR-14 继续待完成。


## 2026-09-19 共享解码与契约矩阵增量

按共享解码 → MIR/helper → 系统模式 → FP/SIMD 顺序补齐本次增量。
详细行为、验证计数和基线边界见 [共享解码、CPU 契约、系统模式与 FP 验收](ir-contract-matrix.md)。
目录更新为 935 编码/3,972 形式；生产 Pending 3,728、实验 Pending 0。
新增便携核心、主线程/Worker fallback URL 和常用 SIMD helper 导入审计。
本次没有将工作负载/性能、默认切换或 IR-14 旧路径退役标记完成。

## 2026-09-19 XP 冷启动性能排查

使用提供的 XP 系统盘、独立实例及内存写入覆盖层复现旧 IR 前 30 秒约
15.78 mIPS。补齐自动编译预算退让/直线路径，减少 Tier 1 编译工作，扩大并索引缓存，
索引热度记录并固定槽位替换，接通已有安全 RAM 写入后继续执行的 CFG 路径。
更新后同一诊断约 25.30 mIPS，首次 640×480×4 模式切换从 35.51 秒降至 17.78 秒。
legacy 最终对照仍约 129.81 mIPS；Node 无渲染诊断不能替代完整浏览器 XP 验收。
差分、缓存失效、精确异常、主线程/Worker 与无 SIMD 测试结果及测量边界见
[IR-13 性能排查](ir13-tuning.md)。IR-13 性能验收和 IR-14 继续待完成。

## 2026-09-19 连续执行、自动多入口和访存复用

- 普通 IR 出口现在实际请求 IR→IR 连续执行，每批最多 64 个后继，并受 CPU 原有
  指令预算约束。每个入口保留完整 admission；故障、预算、慢访存、I/O 和中断
  shadow 出口返回外层调度。链接执行累计热度，但不在链内编译。
- 同一不可变快照可自动编译主入口与一个已热邻近入口；产物是独立守卫的函数，
  浏览器串行发布。配置、SMC、reset/restore 会取消排队产物，发布前再次校验。
- 安全普通 RAM 标量写入后不再立即全量写回状态，而在下一观察点/故障/出口物化。
  Tier 2 增加同一地址、访问范围与权限兼容的 RAM guard 复用；仍执行真实数据访问，
  慢路径在页表遍历和 MMIO callback 前清除缓存。证书由 verifier 独立重算。
- 新低层 `ir_request_link` 只设置请求标志，低层 import 总数为 26。公开统计增加
  `batched_entries` 与 `queued_entries`。
- 220 项原生测试通过；新增实际链接/预算/冷写入/I/O 退出、真实排队兄弟入口取消、
  混合宽度 RAM/跨页故障/设备重映射测试。缓存/自动编译在 debug/release 不变量核心、
  生产 IR 核心及无 SIMD 自动编译路径通过；浏览器主线程和 Worker 回归通过。

这仍未实现跨区域寄存器直接传递、跨 backend 直连、任意多入口区域合并或通用别名
分析；不能据此宣称 IR-00～IR-14 全部完成或已经超过 legacy。

## 2026-09-19 快速入口校验

- 已接入受控同步区间的字节校验证书：同一 epoch 可复用完整校验结果，但每次仍检查
  精确入口、VM generation、源页 TLB 映射和 CPL 读取权限。新 CPU 批次、解释器回退、
  观察型 import、代码失效和 reset 撤销证书；不会把固定 snapshot version 当成真实页版本。
- 热取指已有可见映射且 epoch 未变时复用前置检查，省略重复的取指后字节比较；
  冷取指保留 A-bit/PTE-code alias 检查。64 槽入口缓存减少 BTreeMap 查找，回收压缩时清空。
- 新增 `ir_admission_barrier` 契约和 fast/full/post-fetch/target 命中统计；冷点诊断开关
  可在同一核心内比较两种校验策略。详情及测试命令见 [快速入口校验](ir-fast-entry.md)。
- 220 项原生测试通过。缓存矩阵新增 recording 开/关下的校验 A/B、真实指令计数、
  回调直接改代码及回调改映射；debug/release、无 SIMD、浏览器主线程/Worker 回归通过。

## Hot region fusion follow-up

The experimental runtime now fuses two profiled source regions into one guarded
Tier 2 SSA activation, retaining GPR/FLAGS/XMM and retirement state over internal
edges. Publication, admission and SMC guards cover both snapshots. See
[the fusion contract and tests](ir-fusion.md) for bounds, diagnostics and results.

## Timing and exit diagnostics (2026-09-20)

Opt-in exclusive CPU-batch sampling, exact exit/admission/chain counters, compiler
phase timing and separate asynchronous publication latency are implemented. Public
API and Worker RPC return copied reports. See [diagnostic contracts and results](ir-diagnostics.md).
This adds attribution, not an IR-13 performance pass or an IR-14 default switch.

## XP 与固定 CPU 验收进展（2026-09-20）

本轮补齐编译子阶段/最长调用、helper 退出类别和解释器热点统计；减少干涉图和
CFG 合并的重复工作，增加 liveness 预算、RMW 连续执行、重叠快照、受限四区域
合并、选择性 XMM 同步和有限值 SIMD 路径、实模式/VM86 段切换及 CLI 连续执行。
代码写入与观察点失效分开，映射仍逐入口核验；缓存默认容量保持 256。

后续减少已优化区域的重复热度记录，并缓存未编译入口的查找失败；发布和压缩记录
会清除此类缓存。三次同核复测：固定 CPU 几何平均为 legacy 的 1.075 倍，四项均
不低于 0.9 倍；XP 首次 800×600×32 里程碑为 IR 31.943 秒 / legacy 18.627 秒，
吞吐 48.258 / 117.771 mIPS，**未达到目标**。这是 Node 下的显示模式里程碑，
不是浏览器完整桌面就绪时间。四区域含 helper 的扩展因 XP 回退而禁止发布，
保留已工作的较短区域；隐藏状态/观察点的更大范围合并仍是缺口。
详情和限制见 [IR-13 排查与验收](ir13-tuning.md)。IR-13、IR-14 仍未完成。

## IR-00～IR-12 后续补缺（2026-09-20）

新增可执行的公开优化级别、17 类 pass 禁用及主线程/Worker/页面启动参数接入；
新增经仿射 RMW ticket 证明的提交后读取复用，保留故障、MMIO 和代码别名退出。
231 项原生测试、debug/release RMW 差分、真实 Worker 与便携核心测试通过。
固定 CPU 三轮几何平均为 legacy 的 1.092 倍，部分单项仍落后；不是 XP 达标声明。
实现边界、原始日志和仍未补齐的功能见 [本轮补缺记录](ir00-ir12-followup.md)。


## 2026-09-20：公开诊断、owned MIR 校验及 helper 区域扩展

- `ir_verify`、`ir_dump`、`ir_stats` 已贯通 CPU/Worker 和页面 URL；dump 默认关闭、
  有界保留且复制读取，必要 correctness 校验不能被关闭。
- owned MIR 独立校验 SSA 支配/使用、状态表达式、typed locals 干涉、边复制、
  import 与 RAM 证书；在丢弃 HIR 后可运行，并支持 release `every_pass`。
- CLI 正常实模式/内核路径选择性同步；三/四区域允许审计过的 CLI 和寄存器 SSE
  helper。STI、通用内存/host observer 扩展仍保留限制。
- **这些增量不表示 IR-00～IR-13 全部完成**。XP 配对性能和更广泛的观察点续执行
  仍是验收缺口，详见 [本轮记录](ir-debug-helper-followup.md)。


同轮继续消除 lowering 中重复 allocator，以及 Tier-1 预计算 CPU liveness 的成本；
新增终端 CpuExit/CpuRep 的入口等价状态写回裁剪，Tier-2 liveness 直接消费 owned
MIR 的实际观察点需求。独立 verifier 已检查 effect 链、RMW affine ticket、
实际机器定义和 CPU liveness 所需值。236 项原生测试通过，性能仍以配对验收为准。

再新增基本块内 post-CpuReload 的相同 backing 值写回复用、StateId 多使用点交集
和独立证书校验；修复选择性 SSE adapter 的隐含操作数被 CPU liveness 删除的问题。
当前 238 项原生测试及组合差分通过。固定 CPU 工作量三轮几何平均为 legacy 的
1.095 倍，部分单项仍落后；XP 性能验收另行记录，不据此宣布 IR-13 完成。

本轮 XP 三组配对中位数：IR 33.717 秒 / 45.688 mIPS，legacy 19.251 秒 /
110.088 mIPS，两个门槛均未通过。独立采样确认入口/分派和解释器仍是主要成本，
lowering 最长 47.81 ms、机器优化最长 5.47 ms；没有复现长停顿。容量 768 的
单次对照仍需约 33.2 秒，保留默认 256。详细原始数据及统计限制见
[本轮诊断与验收记录](ir-debug-helper-followup.md)。
