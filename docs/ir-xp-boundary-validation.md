# IR 第二轮边界优化：独立验证（2026-09-20）

验证提交：`88b65fa6`（合并 `6b09c61a`）。性能基线为上一轮 `19989ac5` 的
正式 IR Wasm，与此前记录的 SHA-256 一致；不以最早的 `b991dccf` 代替本轮基线。
本次没有更改区域、分配、helper 续执行和共享入口的优化策略。

## CI 失败与修复

用户日志中的 `live.mjs:133`，本地在未改测试时复现同样的 `76n !== 0n`。
旧测试认为 65 条 NOP 会超过 CFG 块数预算；新前端先合并直线片段，合法编译
成功并返回非零任务身份，因此旧断言已经失效。

修复保留并加强实际约束：

- 65 条 NOP 编译成功、快照验证成功；执行在预算内退出，PC 与实际退休数一致，
  寄存器、FLAGS、XMM 和数据不变。执行预算是工作单位，包含 CFG 分派，不能假定
  设置 32 就必须退休恰好 32 条客户机指令。
- 65 个不同条件分支块仍因 CFG 预算拒绝，并清除旧产物。
- 129 条 NOP 仍因已解码指令数量上限拒绝。

继续运行 `ir-live-tests` 又发现 `live_runtime.mjs` 的旧退出预期：合并后的尾部
会在 store 后执行 HLT，四种 tier/优化组合均为 MOV、三轮 INC/DEC/JNZ、store
和 HLT，共 12 条，计数从 `0xFFFFFFFC` 回绕至 8，IP 到 `0x10000F` 且 HLT 置位。
测试原先只预期 11 条、停在 HLT 前；现改为验证完整的新结果。

另外，新 helper 名称 `ir_in_continue` / `ir_out_continue` / `ir_rdtsc_continue`
没有加入诊断分类，导致其失败续执行退出被计入 `other`。现归回原语义分类，并
通过 observer 修改 XMM、迫使续执行拒绝的执行案例验证分类和退休数守恒。
这只修正诊断开启时的归因，不改变诊断关闭时的执行策略。

## 性能统计口径

新的区域内 observer 续执行仍会完整复核代码 owner。仅查看
`cache_full_checks` 会漏掉它；XP runner 现在同时报告：

- 入口完整检查数；区域内 observer 检查尝试数；二者合计的每百万退休指令密度。
- 活跃入口别名、共享函数编译/发布、共享额外入口、observer 拒绝计数。
- 分段累计的 IR 退休数和激活数，正确处理单个 u32 计数器回绕。

检查次数不是检查耗时：一个更大区域可能比较更多字节，因此仍以关闭诊断的
启动/吞吐配对和独立 CPU profile 为性能结论依据。

## 正确性验证

- 原生：247 项通过、1 项显式微基准忽略。
- `make ir-entry-tests ir-live-tests`：debug、release 和无测试钩子的正式核心通过。
- `make ir-cache-tests ir-auto-tests ir-fusion-tests ir-diagnostic-tests`：通过。
- debug/release 的 STI、helper reload、共享入口和融合差分：通过。
- `make ir-decode-contract-tests ir-portable-tests` 及 helper audit：通过。
- production / ir-experimental 的 Wasm `cargo check`，`-D warnings`：通过。
- 浏览器主线程与 Worker 的 backend、诊断、SMC、save/restore 和失败路径：通过。

日志使用 `build/ir-boundary-validation-*.log` 前缀。以上是本地验证结果，不代表
已经提交修改或远端 GitHub CI 已重新运行。
环境为 macOS arm64、Node 25.6.0、Rust 1.93.1；CI 日志为 Linux、Node 24.17.0。
本地已复现相同断言；额外安装 Node 24.17.0 的进程等待数分钟未完成后被停止，
未宣称通过了与远端完全相同的环境矩阵。

## XP 配对结果：整体退化

同镜像、同一 Node/JS、同一配置，独立 VM，三种方案交替串行各三次；关闭诊断
和性能记录，无并发构建/测试。原镜像使用内存写覆盖层，未修改磁盘文件。
里程碑为首次 800×600×32 显示模式，不等价于完整桌面空闲。

| 三轮中位数 | 上一轮 IR | 本次 IR | legacy |
| --- | ---: | ---: | ---: |
| XP 里程碑 | 31.594 秒 | **36.005 秒** | 18.725 秒 |
| 到里程碑平均吞吐 | 52.527 mIPS | **43.561 mIPS** | 112.311 mIPS |
| 到里程碑退休指令 | 16.604 亿 | 15.845 亿 | 21.081 亿 |

本次相对上一轮启动时间增加 **14.0%**，吞吐降低 **17.1%**。本次/上一轮三组
时间分别为 36.908/31.611、36.005/31.594、33.411/30.573 秒，三组均更慢。
九次都到达里程碑；比较脚本退出 1 是性能门槛失败，而不是 VM 启动失败。
时钟轮询导致工作量不同，吞吐变化不能当作严格等工作量加速比。

| 停止时指标的三轮中位数 | 上一轮 IR | 本次 IR | 变化 |
| --- | ---: | ---: | ---: |
| 每次激活退休指令 | 15.08 | 16.03 | +6.3% |
| 每百万退休指令激活数 | 52,753 | 49,837 | −5.5% |
| 每百万退休指令入口完整检查 | 37,108 | 41,820 | +12.7% |
| 含 observer 的完整验证尝试密度 | 37,108 | 41,994 | +13.2% |
| IR 覆盖 | 79.57% | 80.59% | +1.02 个百分点 |
| 发布产物 | 2,394 | 2,480 | +3.6% |
| 缓存淘汰 | 1,155 | 1,211 | +4.8% |

这些计数采样稍晚于显示事件。每列独立取中位数，不能把不同列中位数相除来
恢复某一轮精确值。尤其验证总密度取每轮总和的中位数，不是两项中位数的和。

本次共享函数发布中位数 195、共享编译 199、额外入口 226，停止时活跃别名 274。
说明共享产物确实被实际使用，但覆盖改善有限，发布总量和淘汰并没有下降。
上一轮核心未导出新别名字段，因此日志中的该字段 0 不表示上一轮没有普通入口。

正式核心 SHA-256：

- 上一轮：`4c3c72398fa4705e1bb20314b58e15b61bbf04dd5df886fd6b33d24f32424959`
- 本次（含诊断分类修复）：`212e15e70eeb7fbf79e0f624a495d5f795139c6c1477e0947b0a42a53599ff28`

原始结果：`build/ir-boundary-validation-xp-summary.json`，逐轮 JSONL 使用同名前缀。

## 固定工作量与分配器微基准

暖态、等退休指令数、每个 workload/后端三次；最终架构状态一致：

| 工作负载 | 本次 IR / legacy 吞吐 |
| --- | ---: |
| 整数 | 0.968 |
| RAM RMW | 0.960 |
| 间接区域 | 1.643 |
| SSE 寄存器 | 0.976 |
| 几何平均 | **1.105，通过既有门槛** |

原始结果为 `build/ir-boundary-validation-fixed.jsonl`。不能用这些已经预热的小循环
代替 XP 在线编译、系统交互和区域边界的验收。

独立运行现有区间/图分配器配对微基准，单块 16/32/96 条 INC 的分配阶段中位数：

| 指令数 | 图分配 | 区间分配 |
| --- | ---: | ---: |
| 16 | 293.6 μs | 37.0 μs |
| 32 | 583.9 μs | 72.5 μs |
| 96 | 1817.2 μs | 213.5 μs |

区间算法在适用的合成单块上约快 8 倍，这一局部收益可复现；它不代表完整编译链。
日志为 `build/ir-boundary-validation-allocation-benchmark.log`。

## 独立诊断：新增成本在哪里

新旧各一轮，周期 16 的诊断加 V8 CPU sampling，串行运行。停止时间为基线
34.052 秒、本次 39.964 秒；退休数约 15.414/15.597 亿。守恒检查通过，细粒度
时间外推仍有警告。以下用于定位热点，不代替关闭诊断的三轮配对，也不能简单
相加预测修复收益。

| 测量项 | 上一轮 IR | 本次 IR |
| --- | ---: | ---: |
| V8 准入/分派分组 | 8.074 秒 | 10.891 秒 |
| V8 生成代码及 helper 分组 | 4.239 秒 | 6.162 秒 |
| 编译 pipeline 全调用计时 | 5.527 秒 / 2431 次 | 6.508 秒 / 2456 次 |
| lowering 全调用计时 | 2.351 秒 | 2.689 秒 |
| 其中 HIR 局部分配 | 1.290 秒 | 1.601 秒 |
| machine 阶段 | 0.538 秒 | 0.887 秒 |
| 发射阶段 | 0.302 秒 | 0.421 秒 |

编译分阶段计数当前合并两个 tier，不能从该总量断言 Tier 1 自身变慢。但可以
确定：单块区间分配的收益没有抵消新工作集/区域形态下的总编译成本。

### 1. 融合资格检查进入了高频准入路径

`runtime/cache.rs` 的 `execute_one` 在每个已准入 Tier 2 激活上调用
`fusion_indices(&cache, entry)` 来判断是否记录热度。它先做 published BTreeMap
查找，再检查边、owner、源集合，部分路径还构建 Vec。

包含该函数的采样栈累计由约 **2.8 ms 升至 1640.1 ms**，其自身叶采样为
1462.8 ms。相比原来的记录内廉价谓词，这是清晰的新增热点。优先修复应是
保留廉价热路径提示，将完整融合资格/闭环分析留到实际候选选择；不能取消
发布身份、源码和映射验证来缩短时间。

### 2. 热点 helper 付出检查成本，却大多没有继续执行

`ScalarObserver::finish` 先重新捕获并比较完整控制上下文和 XMM，然后才判断
`no_pending_irq()`。PIC/APIC 判定把任何 IRR 请求都视为阻止续执行，包含可能
被屏蔽或当前不可交付的请求。

采样中三个标量 helper 的累计栈时间由约 **0.406 秒升至 1.994 秒**；新版本
`ScalarObserver` 栈累计约 1.299 秒，其中标量 helper 内 `memcmp` 叶采样约
0.755 秒。它们是包含关系，不能重复相加。

另跑只读 import 探针：记录 helper 返回值，并在拒绝后查询同一只读 IRQ 谓词；
STI 的发射端谓词也单独计数。探针增加 JS 边界，会扰动速度，因此只用于检查
合约命中情况，不采用其时间作为性能数据。

| 探针 | 总调用 | 区域内继续/无 pending | 拒绝/有 pending |
| --- | ---: | ---: | ---: |
| RDTSC | 4,741,903 | **0** | 4,741,903 |
| IN | 4,927,770 | 208,948 | 4,718,822 |
| OUT | 73,231 | 53,600 | 19,631 |
| STI 完成前 pending 查询 | 4,916,522 | 154,579 | 4,761,943 |

RDTSC 和 IN 的全部拒绝样本，在 helper 返回后 `no_pending_irq()` 也返回 false；该条件
足以阻止这些续执行，但这不证明其他前置条件一定通过，也没有区分是哪种 IRQ。
OUT 有 81 次拒绝不伴随该条件。STI 约 96.9% 仍走原慢路径。

因此第三项在当前 XP 热点上没有兑现预期：RDTSC 成功执行后仍全部退回冷路径，
同时新增了大量控制状态复制/比较。应分别审计“存在请求”与“当前需要交付中断”
的语义，并避免已知不能继续的路径先支付昂贵比较；不能简单删除 IRQ 检查。

`observer_rejections == 0` 只表示到达 **owner 字节检查阶段** 的调用没有被拒绝，
不表示整体续执行成功。大量调用已经被之前的 IRQ 等条件挡住。另需注意，本次
STI 慢路径被归类为 flags helper；旧 `interrupt_shadow` 类别减少并不证明边界消失。

探针脚本和数据保留于 `build/ir-boundary-validation-observer-probe.mjs` / `.jsonl`。

### 3. ready 缺失下降，但发现到发布延迟没有下降

| 独立诊断指标 | 上一轮 IR | 本次 IR |
| --- | ---: | ---: |
| 每百万退休指令 ready 缺失 | 7,546 | 5,487 |
| 每百万退休指令 pending 缺失 | 725 | 983 |
| Tier 1 发现到发布平均 | 58.14 ms | 69.10 ms |
| Tier 1 发现到发布最大 | 994.90 ms | 2203.12 ms |
| Tier 2 发现到发布平均 | 132.15 ms | 171.84 ms |

这是保留热度历史的入口延迟，不是第一生存期的无界 PC 历史；包含执行、排队和
异步安装。共享产物与 ready 优先策略确实减少了一部分 ready 缺失，但尚未让
发布延迟整体下降。编译图规模、晋级/融合频度与安装等待需要一起约束。

## 四个目标的判定

1. **连续执行范围：小幅改善，未达到“明显扩大”。** 激活长度 +6.3%，激活密度
   −5.5%，但验证密度 +13.2%，还新增了融合资格热路径成本。
2. **多入口与发布：部分有效。** 真正共享产物被使用，覆盖 +1.02 个百分点，
   ready 缺失下降；发现到发布延迟、产物总量和淘汰没有改善。
3. **helper 契约：正确性案例通过，XP 性能目标未实现。** RDTSC 续执行探针为零
   成功，IN/STI 绝大多数仍退出；检查成本反而明显增加。
4. **Tier 1 编译：局部算法优化有效，整体冷编译目标未实现。** 单块分配快约
   8 倍，但本次 XP 总 lowering/分配/pipeline 耗时均上升。需要 tier 分桶和
   相同 capture 的编译对照进一步分离算法收益与区域变大/候选变化的代价。

## 复现

```sh
make ir-entry-tests ir-live-tests ir-cache-tests ir-auto-tests ir-fusion-tests ir-diagnostic-tests
IR_BASELINE_WASM=build/ir-boundary-validation-baseline/v86-ir-runtime.wasm \
  node tests/ir/performance/xp_compare.mjs /path/to/windowsxp.img build/ir-boundary-validation-xp
node tests/ir/performance/fixed_work.mjs > build/ir-boundary-validation-fixed.jsonl
cargo test linear_allocation_paired_benchmark -- --ignored --nocapture
IR_BOOT_TARGET=desktop IR_BOOT_MS=180000 IR_DIAGNOSTICS=16 \
  node --cpu-prof --cpu-prof-name=ir-boundary-validation-new.cpuprofile --cpu-prof-dir=build \
  tests/ir/performance/xp_boot.mjs /path/to/windowsxp.img ir > build/ir-boundary-validation-new-diagnostic.jsonl
```

基线诊断同上，末尾显式传入基线 Wasm。诊断/采样汇总和差异分别为
`build/ir-boundary-validation-{baseline,new}-{diagnostic,profile}-summary.json` 与
`build/ir-boundary-validation-diagnostic-comparison.json`。
