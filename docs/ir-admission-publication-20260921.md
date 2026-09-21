# IR 热路径与异步发布修复（2026-09-21）

源码基线：`e008f74c437e2ae1c3b063ecd4e20b3a015c75f3`。
本轮本地环境为 Linux x86-64、Rust 1.98.1、Node 24.17.0。
**没有 XP 镜像；以下固定工作量结果不能当作 XP 提速或追平 legacy 的证明。**

## 具体问题与修复

### 1. 普通短区域付出重复的准入管理成本

先在原有 `performance/boundaries.mjs` 复现：相同算术跨四页分成短片段时，
基线 IR 显著慢于 legacy；连续区域则接近持平。融合默认关闭，以隔离普通边界。
本轮没有把 Rust runtime 当作 JavaScript 边界：CPU/runtime 本身也是 Wasm，
问题在每条边实际执行的查找、检查、调用、状态同步，而非实现语言的名称。

`execute_one` 在所有源页都已经通过 CPU-visible TLB 映射检查后，仍释放 CACHE、
执行实际取指、重新加锁及重新找到同一 owner。当前单线程、非共享内存执行区间内，
这个已证明的暖取指不会走页表、写 accessed 位、交付异常或调用宿主。

新增暖准入路径：保持同一 CACHE guard，**仍执行真实 `get_phys_eip`**，随后合并
激活、后继缓存、LRU 和链接统计。代码字节、映射、发布身份、代际验证没有删除。
缺失映射、冷取指、饱和 epoch、关闭 fast validation，以及诊断开启的路径保留
原先解锁取指及取指后复核。诊断路径保守处理计时 import 的宿主可观察性。

同时仅内联小型决策/谓词，避免无回收工作时反复进入导出的 collector；真正的
压缩、别名重建与表槽回收仍走原实现。`ir_cache_stat(34)` 为暖准入累计数。

这不是任意 generated-code chaining，也不宣称跨独立 Wasm 函数保留了 SSA locals。

### 2. 每个 CPU 区域无条件调用两个指针 getter

原后端即使生成纯寄存器运算，也会调用 `ir_tlb_base`，带代码依赖时还会调用
`ir_memory_base`。这些是对生成模块不透明的导入调用，不能依靠宿主消除。

现在依据已验证的 MIR memory/effect plans，只在确实需要时生成调用。
特殊处理 CMPXCHG8B 的 TLB 需求和 RMW commit 的代码页保护需求；普通 load、
store、向量访存以及 alias/SMC 的保护仍保持。新增原生测试分别验证各类导入需求。

### 3. 异步安装已提交，但 CPU 仍继续执行整个批次

`ir_codegen_finalize` 提交后，安装在宿主 Promise continuation 中发生；旧 CPU
仍运行至批次/时间片结束，期间已编译的产物无法立即用于替代解释执行。
初始隔离回归在基线记录了提交后约一个完整批次（100,004 条指令）的额外工作。

`schedule::visit`/`publish` 现在返回“本次新提交”事件，传递到 CPU 批次和主循环。
在冷安全点交还宿主前仍执行原来的计时器/IRQ 流程。没有使用“只要 pending 就
不断 yield”的状态条件：挂起、失败或取消的 Promise 不会阻塞客户机前进。

另外，旧调度器无候选的空扫描也消耗本帧唯一编译额度。现在区分 `scan_credit`
与编译 `credit`：全表扫描仍每帧至多一次，但当前解释入口达到热度后可使用尚未
消耗的编译额度。没有改变热度阈值、区域大小、执行预算和融合策略。

`publication_yield.mjs` 验证首次空扫描不阻止本帧编译、提交后交还前没有多退休
客户机指令、挂起安装时仍至少推进完整批次，以及成功/拒绝/取消与 u32 退休回绕。
已接入 `ir-auto-tests`，覆盖 debug、release 和无测试钩子的正式核心。

## Node 24 的既有 x87 CI 故障

在未改相关策略的基线上复现了 `x87 case 121/1` 的 NaN payload 差异。精确 F80
测试没有显式关闭浏览器默认的近似 binary64 算术/缓存；这种模式不能用来证明
所有 precision/rounding 组合的精确 F80 行为。

测试现在显式 `x87_fast_math:false, x87_jit_cache:false`，保留所有精确字节比较。
此外 `F80::ln` 对 NaN、零、无穷和负数域显式处理，避免 NaN 经过宿主浮点运算后
在不同 Wasm tier 下改变 sign/payload；保留原有 binary64 转换及其 SoftFloat flags。
这并未实现新的精确超越函数算法，也没有改变浏览器默认的近似模式。
新增 FYL2XP1 的 qNaN/sNaN payload 和负数域 indefinite 独立预期值检查，错误信息
同时记录 sample/precision/rounding。Node 24 debug/release 全矩阵通过。

## 关闭诊断的固定工作量结果

同一 Node/JS、独立 VM、串行交替各三次；无并发构建或回归。
基准自身验证退休指令、GPR、FLAGS、最终 PC、结果内存及保护字。

### 暖态普通边界：4,000,000 次算术更新

| 区域形态 | 基线 IR 中位毫秒 | 本次 IR 中位毫秒 | IR 吞吐变化 |
| --- | ---: | ---: | ---: |
| 连续区域，每片段 3 条算术 | 111.266 | 111.289 | -0.02% |
| 同页跳转，每片段 3 条算术 | 121.414 | 120.526 | +0.74% |
| 跨页短区域，每片段 3 条算术 | 441.481 | 351.130 | **+25.73%** |
| 连续区域，每片段 12 条算术 | 110.574 | 110.471 | +0.09% |
| 同页跳转，每片段 12 条算术 | 117.186 | 113.667 | +3.10% |
| 跨页短区域，每片段 12 条算术 | 186.640 | 171.376 | **+8.91%** |

跨页 3 条组的三轮 IR 基线为 466.49 / 441.48 / 434.26 ms，
本次为 351.13 / 349.44 / 371.08 ms。**即便改善后，该刻意切碎且关闭融合的
工作负载仍只有本次 legacy 吞吐的 33.75%；没有宣称普通边界问题已完全解决。**
跨页 12 条组约为 legacy 的 63.69%。原始结果：`final-boundaries.jsonl`。

### 新增冷发布基准

32 个分离页面依次运行，严格完成 12,582,978 条客户机指令，不做客户机代码预热。

| 中位数 | 基线 IR | 本次 IR | 基线 legacy | 本次 legacy |
| --- | ---: | ---: | ---: | ---: |
| 毫秒 | 451.181 | 422.922 | 250.767 | 258.631 |
| mIPS | 27.889 | 29.753 | 50.178 | 48.652 |
| IR 退休覆盖率 | 13.85% | 20.36% | 0 | 0 |

IR 中位吞吐提高约 6.68%，时间减少约 6.26%。三轮本次/基线为
422.92/470.18、421.40/451.18、452.74/437.69 ms，**第三轮变慢**；legacy 也有
相当波动。因此这是有限的冷态证据，不能声称稳定的大幅收益，更不能推算 XP 秒数。
原始结果：`final-cold.jsonl`。可选 `IR_COLD_PROBE=1` 记录提交到安装的退休工作和
延迟，但有额外观测开销，不用于正式时间比较。

原有 `fixed_work.mjs`：整数 0.923、RAM RMW 0.968、间接区域 1.743、SSE 0.972，
几何平均 IR/legacy = 1.109，既有门槛通过。这不替代边界或系统性能验收。

### 正式核心指纹

- 基线：`180c3b8773ead21a6d260c05798d301eef7ab3bbbe856aade813ae2c67df34a6`
- 本次：`3d40e3336bac69a64df26711d55fdd826b0edbdeecc8985617659a5b80b595ad`

这是本轮 Linux/Rust 工具链构建，不能与先前 macOS 的 Wasm 指纹混用。

## 复现与剩余验收

```sh
RUSTFLAGS='-D warnings' cargo test
make ir-entry-tests ir-live-tests ir-cache-tests ir-auto-tests ir-fusion-tests ir-diagnostic-tests
node tests/ir/differential/x87.mjs
IR_BOUNDARY_UPDATES=4000000 node tests/ir/performance/boundaries.mjs build/v86-ir-runtime.wasm /path/to/e008f74c.wasm
node tests/ir/performance/cold_publication.mjs build/v86-ir-runtime.wasm /path/to/e008f74c.wasm
node tests/ir/performance/fixed_work.mjs
IR_BASELINE_WASM=/path/to/e008f74c.wasm node tests/ir/performance/xp_compare.mjs /path/to/windowsxp.img build/ir-admission-xp
```

本轮原生 250 项通过，2 项显式微基准忽略。正式核心缓存/自动发布/融合/共享入口
生命周期回归及新增提交交还测试已通过。完整 PR CI 的结果以对应提交的 Actions 为准。

仍需 XP 同镜像无诊断配对，关注编译/发布等待、解释覆盖、每百万退休指令的边界与
验证开销。独立区域间仍物化状态，剩余准入、源码复核、编译量和解释执行仍有成本。
不能把这里的局部收益写成已经达到设计目标，也不能从预热小循环推导 XP 已超过 legacy。
