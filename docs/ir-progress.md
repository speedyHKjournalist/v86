# IR-00–IR-14 实施状态

更新：2026-09-13。固定基线：`8ee73e538daaab15411344d39a1f271e778ac7f3`。

**完整请求尚未完成。** 生产默认仍为 legacy；显式选择 IR 时已有自动编译与
Tier 2 升档。本轮保留并整合并行合入的 Tier 2 纯 SSA LICM，补充位精确 SIMD 化简及回归入口，
尚未完成纯 IR 的 Windows XP 启动、全 ISA 或性能验收。
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
| IR-11 | 部分完成 | 已实现有界、事务式纯 SSA LICM 和位精确 SIMD 恒等式/字节重排组合；保留 CFG、预算退出、访存、helper 和异常顺序；proof-based 访存复用、forwarding、归纳变量/强度削减及性能验收仍待实现 |
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

## 本轮推进：IR-11 纯计算优化与可重现回归

- LICM 只使用已有唯一无条件 preheader；识别多回边及嵌套自然循环，跳过
  多入口/不可约循环和循环携带值。使用显式纯操作白名单，不移动 CPU 读取、
  访存、helper、除法、SSE 守卫、状态物化或预算检查。预算失败和 verifier
  失败均不修改输入；在线编译仅 Tier 2 可启用，优化开关可以独立关闭。
- SIMD pass 消除位向量 AND/OR 自身及恒等重排，按字节组合至多两个实际输入的
  重排链；四个独立源不能非法合并为两个源。所有恢复映射一起改写，不改变浮点、
  guest 内存、SSE 资格检查或 CPU ABI。
- 新增 13 项 Rust 优化测试，独立模型执行 7,680 次 LICM 和 9,088 次 SIMD
  reference/optimized Wasm 对照。整合并行更新前完整 native Rust 测试为 140 项，全部通过；
  现有独立 Wasm 执行矩阵亦通过。详细环境、范围和未运行项目见
  [IR-11 实现与验证](ir-optimizations.md)。
- `tests/ir/run_regressions.py` 提供优化子集与 42 个 Make target 的完整回归入口。
  完整入口要求真实固定基线 Git 对象，不在缺失历史时跳过 oracle；CPU feature
  变体串行构建，避免 Cargo 共享输出路径竞态。CI 将优化和 CPU 回归分为两个 job，
  保存诊断日志。添加 CI 不代表尚未观察到的运行结果通过。
- 旧 SIMD 大矩阵一次性保留约六万个 Wasm 模块，在当前环境遇到代码空间耗尽。
  测试改用有界分进程分片，保留每个原始用例与 debug/release、优化开关组合，
  不是抽样、减少覆盖或忽略失败。

原有详细推进记录完整保存在 [本轮之前的状态快照](ir-progress-baseline-20260913.md)。
该文件为历史记录，本页与 `ir-optimizations.md` 描述本轮新增实现。

**未改变的验收门槛：** 不把实验覆盖改成生产完成，不绕过 `ir-default-gate`，
不删除 legacy 发射器，不宣称 XP/游戏兼容或性能提升。完整目标仍为 IR-00–IR-14。

## 并行更新整合

目标 `ir` 在本轮期间前进至 `00da882962df72b4c39e8dd28f9f5a3bb1ac8639`，
已合入另一版 LICM。本分支保留该实现、原测试及 `run_tier2`/统计字段契约，
将本轮 9 项 LICM 测试作为独立补充模块保留，并接入 4 项 SIMD 测试、
按 pass 关闭 LICM 的开关和后续 GVN/DCE 清理；没有用本轮初版覆盖并行工作。

整合后的完整 native Rust 测试为 **153 项通过**，独立 Wasm 矩阵通过；
保留的 LICM oracle 完成 21,600 次执行，新增 LICM/SIMD oracle 分别完成
7,680/9,088 次执行。重新构建的 release runtime 核心通过 live/cache/auto/backend
回归。此前两个 SIMD 大矩阵的全量结果属于整合前版本，最终合并版本的
完整 CPU 矩阵以 PR CI 为准，没有将旧结果冒充为本次重新执行。
