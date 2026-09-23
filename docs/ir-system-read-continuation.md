# CPUID 与 CR/DR 读取的 IR 续执行

本增量补齐 IR-04/IR-07 的三个不必要终端边界。`MOV r32, CRn`、
`MOV r32, DRn` 和无宿主观察的 `CPUID` 使用已有 scalar `CpuReload` ABI，
成功后重载 14 项 GPR/FLAGS backing，保留 XMM SSA，并继续执行后续 CFG。
自动只读快照选择同时包含这些指令的后继；CR/DR 写入和 MSR 操作保持终端。

## 状态与异常契约

- 成功的 CR/DR 读取只修改一个完整 32 位 GPR。CPL、CR 索引、DR4/5 与
  CR4.DE 的检查仍使用原 CPU 实现；忽略 ModRM.mod 和操作数前缀的既有行为不变。
- 正常续执行返回 `Normal`，helper 不退休；最终 StateMap 或预算出口统一计数。
  已派发异常返回 `ControlTransferred`，故障指令不退休。旧的终端导出仍兼容：
  在相同成功结果上退休一次并返回 `Invalidated`。
- `CPUID` 在 release 中没有宿主日志；debug 中仅 leaf 0、2、0x80000000
  没有日志，其余 leaf 在执行前选终端适配器。终端适配器在日志回调前撤销
  admission 证书，完成后保留回调改写的 GPR/XMM/FLAGS、代码、上下文和计数。
  不把日志返回后的状态重新覆盖为原 SSA，也不重复执行 CPUID。
- helper registry 继续使用保守 effects，不把这些指令登记为纯运算。
  `Normal` 的无访存/无宿主回调保证只用于保留已有代码校验证书；故障和
  debug observer 出口仍撤销它。没有改变 CPUID 内容、TSC 或预算单位。

## 验证

入口是 `make ir-control-regs-tests ir-cpu-info-tests`。
新增 `system_read_continuation.mjs` 比较普通/优化 CFG、16/32 位模式、
1～4 条精确退休前缀、计数回绕、CPL/非法 CR/DR alias 故障和后继对输出 GPR
的实际使用；另有 debug CPUID 日志变更状态的对照与直接 helper ABI 计数检查。
原有 CR/DR 和 CPUID/MSR/TSC 形式、映射、部分提交矩阵保留。

本轮 stage3 debug/release 各通过 3,584 个续执行、故障、预算与计数回绕对照；
debug 另通过 96 个 CPUID 日志回调案例，包括 unknown leaf 两次日志之间的
状态改写。直接终端/续执行 ABI 各通过 47/55 个对照，明确核对成功的单次/
延后退休以及故障零退休。原 CR/DR 矩阵在两种核心上各通过 7,680 个普通形式、
1,792 个权限、64 个 DR alias、128 个非法 CR、64 个 CR4 bit、168 个实模式、
14 个 TLB、60 个 PDPTE RAM/MMIO、96 个 PDPTE bit、18 个基线中止策略及
4 个新地址空间取指缺页案例。原 CPU-info 矩阵在两种核心上也通过。
日志为 `build/ir-next-stage3-system-read.log`、`build/ir-next-stage3-control-regs.log`
和 `build/ir-next-stage3-cpu-info.log`。

原 CR/DR 脚本在同一个 Wasm 核心上连续捕获数百次预期 Rust panic；本轮
复现其后续无关 CR4 写入在 `dlmalloc` 中越界。Wasm trap 不会展开 Rust 的
线性内存栈，也不保证 allocator 状态可恢复。测试现对每个预期宿主中止使用
新 VM/core，仅复用不可变编译模块，并跨实例比较复制出的完整架构状态。
没有清空 guest fault、改变预期中止、恢复私有栈指针或删减原测试矩阵。

## 审查中保留的缺口

1. helper 通用逐字段 `reads_state`/`writes_state` 合约仍未落地。当前 registry
   要求保守 effects；pure helper 裁剪与个别 SSE 寄存器特化不足以代替实施计划
   第 6 节中的通用 CPU state 同步证明。这三个 helper 仍重载完整 scalar 状态。
2. POPF 的 debug trap-flag 日志与 IF-enable IRQ 派发、受保护模式段加载、
   selector 查询、x87 memory/environment 和 REP 都仍有终端边界。
   REP 已复用有快路径的有界 CPU string engine，不能称为缺少 REP 语义。
3. 当前 RAM 证明以相同地址和可证明不相交的常量范围为主；循环缓存不覆盖
   含 store/RMW/vector memory/未知观察 helper 的一般循环。通用范围证明、
   归纳变量和有限强度削弱尚未成为一般优化能力。
4. 受守卫区域融合不等于跨任意区域的寄存器直接传递；复杂观察 helper 的
   多源扩展仍受白名单限制。实验粗粒度 Pending=0 也不等于完整前缀、模式、
   异常组合、浏览器 XP 桌面及应用验收完成。

这是功能与正确性增量；实际 XP 的收益必须由同批、关闭诊断的配对测量决定，
不能用续执行覆盖或测试通过宣称 IR-13/IR-14 完成。
