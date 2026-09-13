# IR-10 / IR-11：有界数据流简化与保守 LICM

基于 `ir` 的 `372ccdc42cc8cb66c61c283295ab7ab673b73f2f`，2026-09-13。
这是完整 IR 迁移中的增量实现，不是 IR-00～IR-14 完成声明。
默认后端、生产覆盖状态、快照格式及 graphics proxy 协议均不变。

## 实际接入

`passes::run` 的每轮执行：直线块合并、trivial phi、标量/SIMD 简化、
常量折叠、不可达裁剪、GVN、DCE。最后执行一次 LICM。
所有现有优化编译入口继续调用这条管线；`compile_inner` 强制 Tier 1
跳过 LICM，Tier 2 按配置执行。`optimize=false` 或 `rounds=0` 不启用新优化。

Rust `PassConfig` 新增 `simplify`、`simplify_work_limit`、`licm`、
`licm_work_limit`。两个 pass 默认允许，默认工作预算均为 1,000,000 次访问。
这不是新增 JavaScript 构造参数；公开配置扩展仍需另行实现。
`CompiledArtifact.passes` 记录标量/SIMD 简化数、循环数、外提次数及访问工作量。
外提次数是 motion 数；同一指令退出两个嵌套循环时计两次。

## 循环不变量外提

从支配关系识别自然循环，合并同一 header 的所有 latch；仅接受既有、唯一、
无条件跳向 header 的 preheader。外部入口、额外入口及不可约循环不做外提。
先处理内层循环，再按支配深度处理定义，不能假定 block arena 的编号就是执行顺序。

允许外提的只有单结果、无恢复/提交元数据、不会陷阱的纯 SSA 整数/向量表达式。
每个操作数必须已在循环外定义并支配 preheader，或已在本轮提前外提。
不会移动 CPU 状态读取、访存、RMW、权限/地址检查、SSE guard、helper、除法、
预算检查或其他观察点；不会新建 preheader，也不修改 CFG 边、StateMap 或提交计数。

指令/值 ID 不变。先在临时 Region 上安排移动，完整 verifier 通过后才发布；
预算耗尽或验证失败时，单次 LICM 调用不改变原 Region。

## 标量和 SIMD 简化

标量处理零/一/全位掩码恒等式、相同操作数、已知 select、匹配位宽的
truncate(extend(x))。移位使用 HIR 已定义的 Wasm 机器计数掩码 31/63，
不误用 I8/I16 的类型宽度作为移位计数掩码。

SIMD 处理位运算/算术的零恒等式、自异或/自比较、常量位运算、字节 shuffle
组合和恒等重排、lane 提取/覆盖以及常量 lane。
嵌套 shuffle 需要三个以上叶子向量时不强行压成双源 shuffle。
只有不重叠的 lane 写入才能旁路；16 位提取必须保留 `& 0xFFFF`，
不能把带高位的 I32 源直接作为 PEXTRW 结果。

替换统一经过 `rewrite_values`，包括指令输入、边参数、恢复专用 FLAGS/XMM、
动态 EIP 和动态指令计数基数。状态/helper/访存观察点不会删除。
单次简化调用也使用临时 Region，预算/类型失败原子返回。
这两个 pass 的事务性不代表整个多 pass 管线具有统一回滚事务。

新增 `Op::VectorConst([u8;16])`，经 HIR verifier、独立 MIR 值计划与机器栈
类型检查，最终由 `WasmBuilder::simd_const` 发出位精确的 `v128.const`。
原 `simd_zero` 复用此方法，输出保持不变；不改变浮点、NaN 或 x87 语义。

## 大型差分矩阵的宿主资源控制

原 packed-integer/shuffle 脚本同时保留整个语料的 Wasm Module/Instance，
在 Linux `vm.max_map_count=65530` 的本次环境中触发 V8 可执行代码空间分配失败。
两个脚本现在使用 `fixtureInstances`，按需编译，LRU 至多保留 128 对实例。
所有 case、调试/发布模式、独立 oracle、异常与回调观察断言原样执行；
不依赖调高宿主 mmap 限制、关闭断言或删除测试。实例仍使用同一 CPU 的导入内存。
新增缓存测试覆盖复用、淘汰、参数拒绝与错误 Wasm 传播。

## 已执行验证

本次容器使用 Rust 1.98.1、Node 22.16.0、NASM 2.16.01。

- 原生 Rust 全部 139 项通过；新增 7 项 LICM、5 项简化语义测试。
- 全部生成 Wasm runner 通过；新增 24,960 次 LICM CPU ABI 执行和
  28,224 次 helper 观察，覆盖预算中断、动态计数、FLAGS 和异常所有权。
- 新增独立 oracle 通过 103,000 次标量恒等式、3,328 次 SIMD 字节/lane
  对照和 3,328 次 SSE 故障恢复观察；包括释放 HIR 后由 MIR 发射。
- 2,670,035 次共享解码器/旧 analyzer 对照通过。
- 调试/发布 CPU 差分涵盖寄存器、FLAGS、访存、栈、近控制、移位、乘除、位操作、
  交换、ENTER、misc、循环、CFG、段/系统栈、字符串、I/O、CPU 信息/系统、
  CR/DR、描述符、选择子查询、VERR、CMPXCHG8B、XMM 传送/整数/立即移位/
  shuffle/transfer/lane/masked、CPU 入口及 raw-ZF 观察；全部通过。
- REP、task-register 固定基线对照、在线编译、缓存、自动升档及公开 Node 后端
  调试/发布/无测试钩子构建通过。生产默认覆盖 gate 仍因 3,728 Pending 拒绝切换。

本地 `ndisasm` 独立反汇编 oracle 首次运行因缺少工具退出，不能算通过；
CI 安装 NASM 包（含 ndisasm）后执行此项。本地 Chromium 页面导航被环境策略
以 `ERR_BLOCKED_BY_ADMINISTRATOR` 拒绝，浏览器主线程/真实 Worker 未能复验，
不能计为通过。不能从 Node 测试推断浏览器、Windows XP 或游戏性能通过。

复现入口：

```sh
make ir-tests
make ir-licm-tests ir-simplify-tests
make ir-analyzer-tests ir-memory-tests ir-cfg-tests ir-simd-integer-tests ir-simd-shuffle-tests
make ir-live-tests ir-cache-tests ir-auto-tests ir-backend-integration-tests
```

首次编译沿用项目 Rust/wasm32、Node、NASM、Clang 和 Java 工具链。
`ir-tests` 会生成其余差分脚本需要的原生测试产物；CI 保存日志而非数十万个
生成 Wasm 文件。新专用测试目标也显式依赖既有 Rust 生成表。

## 仍不具备的能力

没有基于 runtime RAM/权限/别名证明的 load reuse、store-to-load forwarding
或访存 LICM；不能用地址相同或 `ordered()==false` 代替 MMIO/MMU 证明。
没有循环展开、向量化、自动 preheader/入口拆分或不安全 FP 简化。
未补齐剩余 ISA、MMX、严格 x87/F80、通用可变 MIR 图和共享版本/链接图。
未执行 Windows XP/真实游戏/性能验收，也未退役 legacy 发射器或减少 Pending。
