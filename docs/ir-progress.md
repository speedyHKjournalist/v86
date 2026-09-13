# IR-00–IR-14 实施状态

更新：2026-09-13。原始固定基线：`8ee73e538daaab15411344d39a1f271e778ac7f3`。
本轮接续点：`ir` 分支的 `372ccdc42cc8cb66c61c283295ab7ab673b73f2f`。

**完整请求尚未完成。** 本轮新增 IR-11 的保守循环不变量外提与纯 SIMD
表达式简化，并扩展其验证。它们只在启用优化、非零 pass rounds 的 Tier 2
编译中执行；默认后端仍为 legacy，旧 JIT 未退役，XP 和应用性能验收尚未完成。

原始完成条件见[实施计划](v86-ir-implementation-plan.md)。截至本轮之前的
详细实现、实验结果、失败记录及阶段说明已**逐字保留**在
[历史进度记录](ir-progress-before-ir11.md)，没有删除或改写既往测试结论。
本轮算法、边界与复现命令见[IR-11 优化说明](ir-optimizations.md)。

## 工作包状态

| 工作包 | 状态 | 已落地及剩余工作 |
|---|---|---|
| IR-00 | 部分完成 | 固定基线、配置类型、测量入口、公开后端/区域参数、Worker 接入和状态查询；完整工作负载与其余诊断配置仍待实现 |
| IR-01 | Builder 改造已实现 | u32 locals、完整 LEB、结构签名、类型复用与实际 Wasm 边界测试；在线容量属于运行时策略 |
| IR-02 | 部分完成 | 共享 decoder 已用于生产区域分析；复杂非法编码、解释器共用及未知编码回退仍待处理 |
| IR-03 | 部分完成 | HIR/SSA/effect/verifier、独立拥有数据的 MIR、机器计划、边复制、locals 和常量折叠；通用 MIR 图变换、其余动作及栈调度仍待实现 |
| IR-04 | 部分完成 | StateMap、异常所有权、helper outcome ABI 与精确恢复；通用 registry 和隐含状态仍待审计 |
| IR-05 | 部分完成 | 大量整数/FLAGS/条件分支与可达直接 CFG lowering；剩余整数 ISA、多入口及成熟区域选择待完成 |
| IR-06 | 部分完成 | 标量/栈/RMW 访存、精确 MMU/MMIO、部分 LOCK 和提交映射；通用 proof 及剩余访存待完成 |
| IR-07 | 部分完成 | 近控制转移、字符串/I/O、有界 REP 及多种系统指令适配；远转移、其余特权指令和在线 REP 策略待完成 |
| IR-08 | 部分完成 | XMM V128、packed integer、重排、lane 传送、精确访存；MMX、浮点控制、F80/x87 及无 SIMD 降级待完成 |
| IR-09 | 部分基础 | 冷 CPU 入口、动态计数、CPU 内编译、正常分派及可选自动升档；完整 Tier 1 语义和系统验收待完成 |
| IR-10 | 部分基础 | 常量折叠、GVN、phi/DCE、分支裁剪、预算保持合并及 MIR 字面量折叠；其余跨块状态同步优化待完成 |
| IR-11 | 部分完成 | **新增**自然循环纯 SSA LICM、shuffle 组合/恒等消除、lane 读写简化及部分 packed 恒等式；事务式验证与工作预算；proof-based 访存复用、forwarding、其他循环优化和收益测量仍未完成 |
| IR-12 | 部分基础 | 只读快照、依赖/映射校验、共享槽池、失效回收、自动编译/抑制/淘汰；完整链接图与版本管理及生产策略验收待完成 |
| IR-13 | 完整矩阵未完成 | 扩展纯值、实际 CPU CFG 和 Tier 配置回归；新增独立 IR CI；完整系统、XP、应用与性能矩阵未完成 |
| IR-14 | 未实现 | 默认后端仍为 legacy，旧 emitter 未退役 |

## 本轮验证入口

```sh
# 常规生成表与构建环境准备后：
RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs
node tests/rust/verify-wasmgen-dummy-output.js
node tests/ir/decode/oracle.mjs
make ir-cfg-tests ir-loop-tests ir-memory-tests ir-backend-integration-tests
make ir-cache-tests ir-auto-tests
```

`tests/ir/wasm/run.mjs` 现在包含 LICM 与 SIMD 简化测试；缺失 Rust 生成的
Wasm 样例会明确失败，不会跳过。`IR core` workflow 保存准确源代码版本和日志，
其 CPU job 在 native/Wasm 测试通过后执行选定 CPU/缓存/自动调度回归。
**命令与 workflow 的存在不是某次执行已通过的证明**；具体提交的结果见其 CI
与 PR 验证记录。历史上的浏览器/Worker 测试不自动视为本轮重新执行的结果。

覆盖目录的 864 条编码记录、3,830 个粗粒度形式与 102 个 baseline-UD 形式
保持不变，**3,728 个生产形式仍为 Pending**。本轮没有通过改写覆盖标签、
降低验收标准或放宽 verifier 来推进 IR-14；默认切换 gate 仍应拒绝当前覆盖状态。
