export const mockWorkspace = {
  roots: [
    {
      id: 'root-1',
      name: {
        en: 'Research Workspace',
        zh: '研究工作区',
      },
      path: '/Users/tanghui/Documents/Papers',
      folders: [
        {
          id: 'folder-ml',
          name: {
            en: 'ML Research',
            zh: '机器学习',
          },
          docs: [
            {
              id: 'resnet',
              title: 'Deep Residual Learning for Image Recognition.pdf',
              shortTitle: 'ResNet.pdf',
              status: 'indexed',
              statusTone: 'success',
              lastOpened: {
                en: '5m ago',
                zh: '5 分钟前',
              },
              pageCount: 32,
              currentPage: 12,
              chatModelId: 'openai-gpt-5-4',
              quoteBlockId: 'resnet-p12-b2',
              chatReady: true,
              translation: {
                status: 'succeeded',
                progress: 32,
                total: 32,
                lang: 'zh',
                error: '',
              },
              pages: [
                {
                  page: 11,
                  heading: '4. Experiments',
                  blocks: [
                    {
                      id: 'resnet-p11-b1',
                      text: 'We evaluate residual networks on CIFAR-10 and ImageNet using plain and bottleneck designs.',
                      translatedText: '我们在 CIFAR-10 和 ImageNet 上评估残差网络，分别测试 plain 与 bottleneck 设计。',
                    },
                    {
                      id: 'resnet-p11-b2',
                      text: 'Optimization becomes difficult as network depth increases without residual shortcuts.',
                      translatedText: '如果没有残差捷径，网络深度增加后优化会明显变难。',
                    },
                  ],
                },
                {
                  page: 12,
                  heading: '4.1 ImageNet Results',
                  blocks: [
                    {
                      id: 'resnet-p12-b1',
                      text: 'Deeper neural networks are more difficult to train. We present a residual learning framework to ease the training of networks that are substantially deeper.',
                      translatedText: '更深的神经网络更难训练。我们提出残差学习框架，以缓解训练显著更深网络时的难题。',
                    },
                    {
                      id: 'resnet-p12-b2',
                      text: 'The degradation problem suggests that simply stacking layers does not guarantee better training accuracy.',
                      translatedText: '退化问题说明，简单堆叠更多层并不能保证获得更好的训练精度。',
                    },
                    {
                      id: 'resnet-p12-b3',
                      text: 'Residual connections enable the optimization of very deep models and improve top-1 performance on ImageNet.',
                      translatedText: '残差连接使得超深模型可以被有效优化，并提升了 ImageNet 上的 top-1 表现。',
                    },
                  ],
                },
                {
                  page: 14,
                  heading: '4.3 Ablation',
                  blocks: [
                    {
                      id: 'resnet-p14-b1',
                      text: 'We compare plain and residual architectures with the same number of layers and observe a clear optimization benefit.',
                      translatedText: '我们比较了层数相同的 plain 架构和 residual 架构，观察到明显的优化收益。',
                    },
                    {
                      id: 'resnet-p14-b2',
                      text: 'Residual learning generalizes to deeper settings where plain counterparts fail to converge well.',
                      translatedText: '残差学习能够推广到更深的设置，而 plain 对照模型往往难以良好收敛。',
                    },
                  ],
                },
              ],
              messages: [
                {
                  id: 'm1',
                  role: 'assistant',
                  content: {
                    en: 'You can keep asking about this paper. I will answer only with evidence grounded in the current PDF.',
                    zh: '你可以从当前论文继续提问，我会只基于这篇 PDF 给出带引用的回答。',
                  },
                  citations: [],
                },
                {
                  id: 'm2',
                  role: 'user',
                  content: {
                    en: 'What are the main contributions of this paper?',
                    zh: '这篇论文的主要贡献是什么？',
                  },
                  citations: [],
                },
                {
                  id: 'm3',
                  role: 'assistant',
                  content: {
                    en: 'There are three main contributions:\n1. It introduces residual learning to address degradation in very deep networks.[1]\n2. It shows residual connections improve optimization and ImageNet performance.[2]\n3. It validates the gain through plain vs residual comparisons, not just added depth.[3]',
                    zh: '主要贡献有三点：\n1. 提出残差学习框架来解决深层网络训练退化问题。[1]\n2. 证明残差连接可以让更深的模型更易优化，并提升 ImageNet 表现。[2]\n3. 通过 plain/residual 对比实验证明收益来自架构而非单纯加深层数。[3]',
                  },
                  citations: [
                    {
                      id: 'c1',
                      label: '[1]',
                      page: 12,
                      blockId: 'resnet-p12-b1',
                      quote: 'We present a residual learning framework to ease the training of networks that are substantially deeper.',
                    },
                    {
                      id: 'c2',
                      label: '[2]',
                      page: 12,
                      blockId: 'resnet-p12-b3',
                      quote: 'Residual connections enable the optimization of very deep models and improve top-1 performance on ImageNet.',
                    },
                    {
                      id: 'c3',
                      label: '[3]',
                      page: 14,
                      blockId: 'resnet-p14-b1',
                      quote: 'We compare plain and residual architectures with the same number of layers and observe a clear optimization benefit.',
                    },
                  ],
                },
              ],
            },
            {
              id: 'vit',
              title: 'An Image is Worth 16x16 Words.pdf',
              shortTitle: 'ViT.pdf',
              status: 'indexing',
              statusTone: 'warning',
              lastOpened: {
                en: '2h ago',
                zh: '2 小时前',
              },
              pageCount: 24,
              currentPage: 1,
              chatModelId: 'deepseek-chat',
              quoteBlockId: '',
              chatReady: false,
              translation: {
                status: 'idle',
                progress: 0,
                total: 0,
                lang: 'zh',
                error: '',
              },
              pages: [],
              messages: [],
            },
            {
              id: 'gpt4',
              title: 'GPT-4 Technical Report.pdf',
              shortTitle: 'GPT-4 Technical Report.pdf',
              status: 'stale',
              statusTone: 'danger',
              lastOpened: {
                en: 'Yesterday',
                zh: '昨天',
              },
              pageCount: 98,
              currentPage: 4,
              chatModelId: 'deepseek-chat',
              quoteBlockId: '',
              chatReady: true,
              translation: {
                status: 'failed',
                progress: 14,
                total: 98,
                lang: 'zh',
                error: 'provider timeout',
              },
              pages: [
                {
                  page: 4,
                  heading: 'Capabilities Overview',
                  blocks: [
                    {
                      id: 'gpt4-p4-b1',
                      text: 'GPT-4 exhibits human-level performance on various professional and academic benchmarks.',
                      translatedText: 'GPT-4 在多种专业和学术基准上表现出接近人类水平的能力。',
                    },
                  ],
                },
              ],
              messages: [
                {
                  id: 'g1',
                  role: 'assistant',
                  content: {
                    en: 'The translation job for this document failed, but you can still keep reading and asking questions.',
                    zh: '这篇文档的翻译任务失败了，但你仍然可以继续阅读和问答。',
                  },
                  citations: [],
                },
              ],
            },
          ],
        },
        {
          id: 'folder-math',
          name: {
            en: 'Math',
            zh: '数学',
          },
          docs: [
            {
              id: 'alpha',
              title: 'AlphaGeometry.pdf',
              shortTitle: 'AlphaGeometry.pdf',
              status: 'indexed',
              statusTone: 'success',
              lastOpened: {
                en: '3d ago',
                zh: '3 天前',
              },
              pageCount: 41,
              currentPage: 6,
              chatModelId: 'openrouter-claude-3-7-sonnet',
              quoteBlockId: '',
              chatReady: true,
              translation: {
                status: 'idle',
                progress: 0,
                total: 41,
                lang: 'zh',
                error: '',
              },
              pages: [
                {
                  page: 6,
                  heading: 'Method Overview',
                  blocks: [
                    {
                      id: 'alpha-p6-b1',
                      text: 'AlphaGeometry combines neural guidance with symbolic deduction.',
                      translatedText: 'AlphaGeometry 将神经引导与符号推理结合起来。',
                    },
                  ],
                },
              ],
              messages: [
                {
                  id: 'a1',
                  role: 'assistant',
                  content: {
                    en: 'This is another indexed paper used to demonstrate translation entry and dual-view switching.',
                    zh: '这是另一篇已索引论文，用来演示开始翻译和双栏切换。',
                  },
                  citations: [],
                },
              ],
            },
          ],
        },
      ],
      recents: ['resnet', 'alpha', 'gpt4'],
    },
  ],
}

export const translationLanguages = [
  { value: 'ko', label: { en: 'Korean', zh: '韩语', ko: '한국어' } },
  { value: 'zh', label: { en: 'Chinese', zh: '中文', ko: '중국어 (간체)' } },
  { value: 'zh-TW', label: { en: 'Chinese (Trad.)', zh: '繁体中文', ko: '중국어 (번체)' } },
  { value: 'en', label: { en: 'English', zh: '英语', ko: '영어' } },
  { value: 'ja', label: { en: 'Japanese', zh: '日语', ko: '일본어' } },
  { value: 'fr', label: { en: 'French', zh: '法语', ko: '프랑스어' } },
  { value: 'de', label: { en: 'German', zh: '德语', ko: '독일어' } },
  { value: 'es', label: { en: 'Spanish', zh: '西班牙语', ko: '스페인어' } },
  { value: 'pt', label: { en: 'Portuguese', zh: '葡萄牙语', ko: '포르투갈어' } },
  { value: 'it', label: { en: 'Italian', zh: '意大利语', ko: '이탈리아어' } },
  { value: 'ru', label: { en: 'Russian', zh: '俄语', ko: '러시아어' } },
  { value: 'ar', label: { en: 'Arabic', zh: '阿拉伯语', ko: '아랍어' } },
  { value: 'hi', label: { en: 'Hindi', zh: '印地语', ko: '힌디어' } },
  { value: 'th', label: { en: 'Thai', zh: '泰语', ko: '태국어' } },
  { value: 'vi', label: { en: 'Vietnamese', zh: '越南语', ko: '베트남어' } },
  { value: 'id', label: { en: 'Indonesian', zh: '印尼语', ko: '인도네시아어' } },
]

export const chatModels = [
  {
    id: 'openai-gpt-5-4',
    provider: 'OpenAI',
    label: 'GPT-5.4',
    capabilities: ['text', 'vision'],
  },
  {
    id: 'openrouter-claude-3-7-sonnet',
    provider: 'OpenRouter',
    label: 'Claude 3.7 Sonnet',
    capabilities: ['text', 'vision'],
  },
  {
    id: 'deepseek-chat',
    provider: 'DeepSeek',
    label: 'deepseek-chat',
    capabilities: ['text'],
  },
]

export const defaultChatModelId = 'openai-gpt-5-4'
