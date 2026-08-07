# Design

komo 的视觉世界。产品事实见 [PRODUCT.md](PRODUCT.md)；本文只记录**已经建成**的东西，不写打算做的事。

世界的名字是 **komorebi（木漏れ日）**——树叶间洒落的阳光。它不是装饰母题，而是产品机制的同构物：小片刻积累成恒久之物，正是记忆随时间积累。

## 主题 token

全部定义在 [main.css](apps/app/src/styles/main.css)，light（暖纸 / 苔墨 / 鼠尾草）与 dusk（林荫 + 柔亮叶绿）两套。组件**只能**引用语义 token，`bun run lint` 会拦住裸色值。

新增的一对状态色取自品牌本身，取代了此前借用的 emerald / amber：

| Token | light | dark | 语义 |
|---|---|---|---|
| `--success` | `oklch(0.6 0.115 133.5)` | `oklch(0.79 0.105 133.5)` | 活着、受光 |
| `--warning` | `oklch(0.845 0.142 104.073)` | `oklch(0.87 0.13 100)` | 需要注意、常驻 |

配套 `--success-foreground` / `--warning-foreground` 供着色底上的文字使用。这对 token 补上了主题此前的缺口，`apps/app/README.md` 里「主题没有 success/warning，所以状态色是唯一可用 Tailwind 色阶的地方」这条例外**已经作废**——`badge.tsx` 的 `ok`/`warn` 与侧栏连接点现在都走 token。

## 动效人格

「树荫下安静的朋友」——不惊扰。全部动效用这三个 token，不自创曲线：

- `--ease-komo: cubic-bezier(0.16, 1, 0.3, 1)`（指数缓出，无回弹、无过冲）
- `--ease-komo-soft: cubic-bezier(0.32, 0.72, 0, 1)`
- `--duration-quick: 120ms` / `--duration-base: 220ms` / `--duration-settle: 480ms`

## 材质：叶隙光斑

`.komorebi-dapple` 是这个世界唯一的材质：十处小而分散的柔边光池（叶绿与阳光黄），以 42s 周期极缓漂移，模拟枝叶晃动。

约束是它成为材质而非装饰的原因：

- 画在 `::before` 的负 z-index 上，透明度混入 `transparent`，地色只偏移几个百分点，**不影响文字对比度**；
- 单个光池不超过盒子的 ~14%，alpha 不超过 ~14%——四团大色块会读成模糊图片，不是木漏れ日；
- **只用于低密度区**（空状态、记忆界面头部）。绝不铺在对话记录、表单或控件之下：叶隙光是等待时看的东西，不是隔着它读字的东西；
- 暗色下经 `--dapple-peak: 0.45` 整体压暗（峰值走变量，因为漂移动画本身占用 `opacity`，静态覆盖会输给它）；
- `prefers-reduced-motion` 下停止漂移，静态透明度仍随主题。

`.komorebi-afterglow` 是变化的表达方式：刚变动的行**留住一层暖光再缓慢褪去**，而不是闪烁一下——你移开视线再回来，仍能找到什么动过。

`.komorebi-spinner` 是等待的统一形象：一粒光沿淡色轨道绕行。

外壳保持安静（`.komo-workspace` 顶部一层极淡的主色渐变），暖意来自相邻表面而非特效。

## 光的编码：记忆的在场程度

[features/memory/light.ts](apps/app/src/features/memory/light.ts) 是这套编码的唯一出处。光**不是**装饰，也**不是**「回忆次数 → 亮度」，而是记忆系统自己的阶梯——一条记忆此刻离模型的 prompt 有多近：

| 层 | 含义 | 光 |
|---|---|---|
| 常驻 pinned | 每一轮对话都带上（L1） | 满照，阳光黄 |
| 受光 active | 可被回忆检索到（L2/L3） | 受光，叶绿 |
| 新芽 candidate | 刚抽取出来，等待确认 | 林下微光 |
| 荫影 archived | 不再进入 prompt，仍留在库里 | 荫影 |
| 落叶 rejected | 已否决 | 落叶 |

回忆次数在**层内**叠加，不取代层级：一条从未被回忆的记忆，只要是常驻，就仍在每一轮 prompt 里。

这个选择有实测依据：当前 29 条记忆的 `recall_count` 全为 0（记忆被抽取成英文、用户用中文对话，词法回忆的词项集合永不相交）。若光只映射 recall，整片林冠会是一样的暗。层级是今天真正携带信息的信号，recall 则随积累成为更细的粒度。界面对此**如实说明**，不假装机制在运转。

## 界面语言

- **文案说产品自己的话**：界面用中文，动词说动作（「转为受光」而非 `promote`），术语与 [CONTEXT.md](CONTEXT.md) 一致。枚举值一律映射为中文，映射表与 komo-core 的 `MemoryKind` / `MemoryConfidence` 变体逐一对应。
- **图标是画出来的**：lucide 统一描边，绝不用 emoji 或 Unicode 字符充当图标。
- **操作按需现身**：等待决定的行（新芽）动作常驻可见；其余行的次要动作等指针。
- **规模生存策略**：荫影层默认折叠——几百条记忆时它是库存的大多数，让人滚过它才够到那几条待决定的，正是平铺列表的失败之处。
- 尺寸走既有比例（`text-sm`/`text-xs`、`rounded-md`/`lg`/`xl`），不写魔法数值。

## 已验证

浅色与暗色两套主题下，全部可见文字对比度均达标（正文 ≥4.5:1，大字 ≥3:1，canvas 采样实测 0 处失败）；桌面 1440 与移动 375 两个宽度下无横向溢出；`bun run check`（typecheck + lint + fmt + 89 项测试）通过。
