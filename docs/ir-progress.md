# IR-00–IR-14 实施状态

更新：2026-09-13。固定基线：`8ee73e538daaab15411344d39a1f271e778ac7f3`。

**完整请求尚未完成。** 本次落地了可执行的实验性 IR 编译器与 WasmBuilder
前置改造。生产 Tier 1/Tier 2 仍使用旧 JIT，尚未完成纯 IR 的 Windows XP 启动验收。
原始验收范围以 [实施计划](v86-ir-implementation-plan.md) 为准，没有降低其完成条件。

| 工作包 | 状态 | 已落地及剩余工作 |
|---|---|---|
| IR-00 | 部分完成 | 已固定 SHA、保存 release 基线、建立测试入口、编译器配置类型及初始测量记录；已有公开 legacy/ir 后端选择、区域参数、Worker 传递和状态查询；其余公开调试/优化配置及完整工作负载仍待实现 |
| IR-01 | Builder 改造已实现 | u32 locals、完整 LEB 索引/长度、结构签名、类型复用及真实模块执行边界测试；在线 table 容量仍是运行时策略 |
| IR-02 | 部分完成 | 生产区域分析已消费共享 decoded，2,670,035 例与旧 analyzer/EA 解码逐项一致；补齐 helper 块边界及前缀分派 ModRM 读取规则；未知编码仍回退旧分析器，解释器及复杂非法编码模型尚未共用 |
| IR-03 | 部分完成 | HIR arena、SSA、effect、支配/verifier、dump、可执行 dispatcher、边复制和 local 复用；已将标量/RMW 读取和 XMM 访存的 guard、物理动作、CPU 调用与退出策略迁入 MIR 计划；已增加 RMW 提交、地址/范围检查和 SSE 守卫的 effect 计划；已加入 CMPXCHG8B/除法的边界、寄存器与提交计划及通用 helper 调用点的观察/暂存/出口计划；已加入 lowering 拥有的 dispatcher CFG 和 typed 边复制调度；已加入标量/向量值程序、packed kernel 选择及机器栈类型检查；已加入有序状态物化与独立计数阶段，以及 SSA 动态计数基数；已移除保留的 HIR 副本，加入受校验的 lowering 事务、独立 MIR 所有权和机器常量折叠；通用 MIR 图变换、其余动作、入口拆分和栈调度仍待实现 |
| IR-04 | 部分完成 | StateMap、helper 副作用及异常所有权、可执行 outcome ABI；42 例 Wasm 验证状态物化、快照槽位复用和单次派发；已适配 CPU 分段/安全访存及真实 #PF/#GP；已增加终端 CPU 状态 helper 契约；通用 helper registry、更多隐含状态仍待审计 |
| IR-05 | 部分完成 | 寄存器算术、FLAGS、别名、条件码、移位/旋转、SHLD/SHRD、乘除法及终端分支可生成 Wasm；last_op1 和原始 ZF 来源已进入 SSA/快照；已增加位测试/修改、位扫描、POPCNT、BSWAP、XADD、CMPXCHG、BCD、FLAGS 传送、符号扩展和计数分支；已增加可达直接 CFG 字节 lifting；其余整数指令、多入口和在线区域选择未完成 |
| IR-06 | 部分完成 | MOV/moffs/XLAT、整数 ALU/比较/条件操作及 INC/DEC/NEG/NOT 访存；成对 RMW ticket、原生 RAM 和精确 MMU/MMIO；已增加 PUSH/POP、PUSHA/POPA、LEAVE、ENTER 和多次访存提交；已增加交换指令、原生 RAM CMPXCHG8B 及非共享单线程 ABI 下经审计的 LOCK；已增加 FLAGS/段栈操作、段 MOV 和远指针加载；通用 proof 及其余访存待实现 |
| IR-07 | 部分完成 | 近 CALL/RET、FF /2 与 /4 间接转移及动态 EIP StateMap；已增加单次 MOVS/CMPS/STOS/LODS/SCAS 原生执行；已增加标量 IN/OUT 和单次 INS/OUTS；已接入有界 REP HIR、进度映射及最终提交；已增加 CPUID/RDTSC/RDMSR/WRMSR 终端适配；已增加 SYSENTER/SYSEXIT、HLT/CLI/CLTS/WBINVD；已增加 CR/DR 传送与 CPU 地址映射变更适配；已增加描述符表、SMSW/LMSW 与 INVLPG；已增加 SLDT/STR 与 LLDT/LTR；已增加 LAR/LSL、VERR/VERW；在线 REP 调度、远转移及其余特权/系统指令待实现 |
| IR-08 | 部分完成 | XMM V128 SSA、快照、typed locals/边复制，以及 packed/scalar SIMD 传送的原生 RAM 与精确慢路径已实现；已增加 38 种 packed integer 算术/比较/乘法/逻辑及 PS/PD 逻辑别名；已增加打包/解包、变量及立即数 packed 移位；已增加 PSHUF/SHUF 重排；已增加半部传送、MOVD/MOVQ 和重复 lane；已增加符号位掩码、PINSRW/PEXTRW、非临时存储和 LDDQU；已增加 MASKMOVDQU 原生 RAM 与有序慢路径；其余 SIMD 状态/传送、MMX、FP 控制、F80/x87 和无 SIMD 降级仍待实现 |
| IR-09 | 部分基础 | 整数后端可执行 CFG 和寄存器代码；已增加冷 CPU 入口及真实状态 ABI，可执行具备完整动态计数映射的 CPU 循环；CompileRequest 产物已有显式入口键及执行前校验，实验 CPU Wasm 内可直接执行 IR 编译，显式发布的入口已参与正常 CPU 分派；已有可选的自动热度、区域编译和优化升档；完整 Tier 1 语义、成熟区域选择和系统验收仍未完成 |
| IR-10 | 部分基础 | 有界常量折叠、支配关系 GVN、trivial phi 消除、StateMap-aware DCE、常量分支裁剪和保留预算检查的直线块合并；已有独立 MIR 字面量常量折叠；其余跨块 FLAGS/状态同步优化未完成 |
| IR-11 | 部分完成 | 已加入可关闭、有工作预算及失败回滚的纯标量/向量 LICM，保留有序动作及 StateMap；proof-based 访存复用、forwarding、归纳变量/强度削弱与其余 SIMD 优化仍未完成 |
| IR-12 | 部分基础 | 不可变编译请求及 generation/dependency/入口/映射比较已实现；已有只读 CPU 代码快照、单个未发布产物句柄和重校验；共享在线 legacy 桥接已有票据校验、安装前拒绝、缓存取消和浏览器失败回收；已有共享槽池中的 IR 缓存、物理代码页监视和冷执行帧返回后的回收；已有有界自动编译、失败抑制和自动入口淘汰；完整共享版本/链接图及生产策略验收仍未完成 |
| IR-13 | 完整矩阵未完成 | 已执行 IR 差分和部分生产 legacy/Worker/API 回归；GPU 间歇失败、PIC 跳过等结果有单独记录；已有 Node 中显式/自动 IR 缓存分派及升档测试，以及公开后端在真实浏览器主线程/Worker 的升档、SMC、双向跨后端快照和错误上报测试；完整在线 IR、XP、应用及性能矩阵未完成 |
| IR-14 | 未实现 | 默认后端仍为 legacy，旧 emitter 未退役 |

覆盖目录共 864 条编码记录、3,830 个粗粒度形式，其中 102 个是明确的
baseline-UD reg/mem 形式，**3,728 个生产形式仍为 Pending**。实验前端具有
838 个原生寄存器/EA 形式、622 个实验 CPU 访存形式、156 个实验栈形式、28 个实验近控制形式、14 个 CPU 算术形式、28 个 CPU 状态形式、30 个单次字符串形式、36 个 I/O 形式、84 个 REP helper 形式、8 个 CPU 信息 helper 形式、12 个 CPU 系统 helper 形式、8 个 CR/DR helper 形式、56 个描述符/机器状态 helper 形式、32 个任务/LDTR helper 形式、32 个选择子查询 helper 形式、450 个 XMM SIMD 形式和 152 个终端分支形式的实现能力；这不是对全部
前缀、特权、子编码组合逐一完成测试的声明。JSON 分别记录生产状态与实验状态。

`make ir-default-gate` 在生产 Pending 非零时按预期失败。`ir-experimental`
Cargo feature 允许 IR 入口参与 CPU 分派，并提供可选的自动编译/升档策略；
该策略默认关闭，生产默认后端仍为 legacy。
没有宣称 XP 兼容、游戏加载改善或 IR 提速。

下一依赖：继续完善 IR-02/03/04 契约、MIR 图变换/分配调度及未覆盖 ISA。
下一步需完善区域选择、共享失效图、链接及调度恢复，补齐未覆盖 ISA 和系统/性能验收；
可选的自动 IR 策略不等于完成生产默认 Tier 的全面迁移。

详见 [测试报告](ir-validation.md)、[实现说明](ir-design.md)、
[helper 契约状态](ir-helper-contracts.md) 和 [覆盖说明](ir-coverage.md)。

## 后续推进：受预算约束的纯 SSA LICM

- 在既有优化管线末尾增加自然循环识别和循环不变量外提；仅使用已有、唯一、
  无条件的 preheader，不修改 CFG、预算检查、StateMap 或 helper/访存次序。
- 对嵌套循环采用内层优先次序，合并同一 header 的多个 latch；跳过无合法
  preheader、外部入口循环和不满足单入口条件的区域。CPU backing-state 读取
  不属于可投机运算，不能仅凭 `!Op::ordered()` 决定是否外提。
- `PassConfig.licm` 可单独关闭；`LicmConfig.max_work` 限制工作量。变换先在
  私有副本上完成，验证成功后才提交；失败不改变调用者区域。统计记录自然循环、
  外提次数与工作量。该开关只影响已经请求优化的实验 IR，不改变默认后端。
- 本轮本地验证通过 136 项 Rust 测试、31,104 次新增标量/向量 Wasm 对照，
  以及现有 Wasm 语义套件。新增 oracle 独立计算循环次数、溢出、零次循环、
  恢复 PC 和动态指令计数，并逐字节比较优化前后的状态映像。
- 完整 ISA、通用 RAM proof/forwarding、系统/XP/应用性能矩阵及 legacy 退役
  仍未完成；没有改变覆盖目录中的 Pending 状态或放宽发布门槛。

详见 [LICM 契约与测试](ir-licm.md)。

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
