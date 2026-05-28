use serde::Serialize;

use crate::runtime::rag::Citation;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextToolCall {
    pub tool: String,
    pub args: serde_json::Value,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeDecision {
    pub status: String,
    pub citation_count: usize,
    pub needs_more_evidence: bool,
    pub reason: String,
    pub missing: Vec<String>,
    pub next_tool: Option<String>,
    pub next_tool_call: Option<NextToolCall>,
    pub attempt: u32,
    pub max_attempts: u32,
    pub budget_exhausted: bool,
    pub runtime: String,
}

pub fn finalize_citations(
    question: &str,
    intent: &str,
    citations: &[Citation],
    attempt: u32,
    max_attempts: u32,
) -> FinalizeDecision {
    let judge = AnswerabilityJudge::default();
    let mut decision = judge.decide(question, intent, citations);
    finish_decision(&mut decision, citations.len(), attempt, max_attempts);
    decision
}

#[derive(Default)]
struct AnswerabilityJudge {
    guard: RuleGuard,
    heuristic: HeuristicAnswerabilityJudge,
}

impl AnswerabilityJudge {
    fn decide(&self, question: &str, intent: &str, citations: &[Citation]) -> FinalizeDecision {
        let needs = QuestionNeeds::from_question(question, intent);
        let facts = EvidenceFacts::from_citations(citations);
        if let Some(decision) = self.guard.decide(question, &needs, &facts, citations) {
            return decision;
        }
        self.heuristic.decide(&needs, &facts)
    }
}

#[derive(Default)]
struct RuleGuard;

impl RuleGuard {
    fn decide(
        &self,
        question: &str,
        needs: &QuestionNeeds,
        facts: &EvidenceFacts,
        citations: &[Citation],
    ) -> Option<FinalizeDecision> {
        if facts.has_selection {
            return Some(answerable(
                "User-selected text is direct evidence for the question.",
                "m3-rule-guard",
            ));
        }
        if needs.header && (facts.has_header || facts.has_author_evidence) {
            return Some(answerable(
                "The retrieved document header contains the requested paper metadata.",
                "m3-rule-guard",
            ));
        }
        if needs.reference && facts.has_reference {
            return Some(answerable(
                "The retrieved evidence contains reference or related-work context.",
                "m3-rule-guard",
            ));
        }
        if needs.reference {
            return Some(needs_more(
                "The question asks for references or supporting source context, but no reference-related evidence is available.",
                vec!["reference or related-work evidence"],
                NextToolCall {
                    tool: "open_section".to_string(),
                    args: serde_json::json!({
                        "query": "references related work citation bibliography source prior work",
                        "perSectionLimit": 8
                    }),
                    reason: "Open references or related-work sections for source evidence."
                        .to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if needs.overview
            && facts.has_overview
            && facts.evidence_chars >= 280
            && overview_evidence_can_answer(question, needs)
        {
            return Some(answerable(
                "The retrieved page overview contains enough title/abstract context.",
                "m3-rule-guard",
            ));
        }
        if needs.overview && !facts.has_overview {
            return Some(needs_more(
                "The question asks for a document-level overview, but no page overview evidence is available.",
                vec!["title and abstract"],
                NextToolCall {
                    tool: "open_pages".to_string(),
                    args: serde_json::json!({ "page": 1, "mode": "overview" }),
                    reason: "Open the first-page overview for title and abstract context."
                        .to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if needs.location && has_location_evidence(citations, asks_section_location(question)) {
            return Some(answerable(
                "The retrieved evidence identifies the requested page or section location.",
                "m3-rule-guard",
            ));
        }
        if needs.location {
            return Some(needs_more(
                "The question asks where content appears, but the available evidence does not identify a page or section strongly enough.",
                vec!["page or section location"],
                NextToolCall {
                    tool: "open_section".to_string(),
                    args: serde_json::json!({
                        "query": "section page location introduced described definition method reference",
                        "perSectionLimit": 6
                    }),
                    reason: "Open likely sections so the answer can cite a concrete location."
                        .to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if let Some(table_number) = requested_table_number(question) {
            if has_open_table_evidence_for_number(citations, &table_number)
                || has_current_view_table_evidence_for_number(citations, &table_number)
            {
                return Some(answerable(
                    "The requested numbered table is available in structured or current-view evidence; semantic sufficiency must be checked by the LLM evidence judge when available.",
                    "m3-rule-guard",
                ));
            }
            return Some(needs_more(
                "The question asks about a specific numbered table, but that full table has not been opened yet.",
                vec!["full requested table evidence"],
                NextToolCall {
                    tool: "open_table".to_string(),
                    args: serde_json::json!({
                        "tableNumber": table_number,
                        "query": question,
                        "limit": 40
                    }),
                    reason: "Open the requested numbered table before generic definition or broad context search.".to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if needs.method && !facts.has_method {
            return Some(needs_more(
                "The question asks about the method or framework, but no method-section evidence is available.",
                vec!["method section"],
                NextToolCall {
                    tool: "open_section".to_string(),
                    args: serde_json::json!({
                        "query": "method approach methodology algorithm framework",
                        "perSectionLimit": 10
                    }),
                    reason: "Open the method-related section for grounded method evidence."
                        .to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if needs.method && needs.experiment && !facts.has_experiment {
            return Some(needs_more(
                "The question asks to connect method design with experiments or results, but no evaluation-section evidence is available.",
                vec!["experiment or result section"],
                NextToolCall {
                    tool: "open_section".to_string(),
                    args: serde_json::json!({
                        "query": "experiments evaluation results benchmark",
                        "perSectionLimit": 10
                    }),
                    reason: "Open the experiment/result section so the method answer can cite evaluation evidence.".to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if needs.experiment && !facts.has_experiment {
            return Some(needs_more(
                "The question asks about experiments or results, but no evaluation-section evidence is available.",
                vec!["experiment or result section"],
                NextToolCall {
                    tool: if needs.table {
                        "search_table_facts".to_string()
                    } else {
                        "open_section".to_string()
                    },
                    args: serde_json::json!({
                        "query": "experiments evaluation results benchmark metric score SOTA table",
                        "perSectionLimit": 10,
                        "limit": 16
                    }),
                    reason: if needs.table {
                        "Search structured table facts for benchmark scores and metrics."
                            .to_string()
                    } else {
                        "Open the experiment/result section for evaluation evidence."
                            .to_string()
                    },
                },
                "m3-rule-guard",
            ));
        }
        if needs.table && !facts.has_table {
            return Some(needs_more(
                "The question asks for table-like metrics or benchmark scores, but no structured table fact evidence is available.",
                vec!["table facts or benchmark scores"],
                NextToolCall {
                    tool: "search_table_facts".to_string(),
                    args: serde_json::json!({
                        "query": "SOTA benchmark metric score performance results table",
                        "limit": 16
                    }),
                    reason: "Search normalized table facts before relying on prose evidence."
                        .to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if needs.table && facts.has_table && !facts.has_open_table {
            return Some(needs_more(
                "The question asks for table-like metrics or benchmark scores; structured facts were found, but the full table has not been opened for row/column coverage.",
                vec!["full table row and column evidence"],
                NextToolCall {
                    tool: "open_table".to_string(),
                    args: serde_json::json!({
                        "query": question,
                        "limit": 40
                    }),
                    reason: "Open the candidate table so the evidence judge can verify target table, row, and metric coverage.".to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if needs.table && facts.has_open_table {
            return Some(answerable(
                "A structured table has been opened; semantic sufficiency must be checked by the LLM evidence judge when available.",
                "m3-rule-guard",
            ));
        }
        if needs.figure && !facts.has_figure {
            return Some(needs_more(
                "The question asks about a figure or table, but no figure/table evidence is available.",
                vec!["figure or table caption"],
                NextToolCall {
                    tool: "inspect_visuals".to_string(),
                    args: serde_json::json!({
                        "query": "figure table chart caption",
                        "limit": 8
                    }),
                    reason: "Inspect indexed visual assets and captions.".to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if facts.has_current_view && facts.evidence_chars >= 280 {
            return Some(answerable(
                "Current-view page evidence is available; semantic sufficiency must be checked by the LLM evidence judge when available.",
                "m3-rule-guard",
            ));
        }
        if needs.definition && has_definition_evidence(question, citations) {
            return Some(answerable(
                "The retrieved evidence contains a definition or introductory explanation.",
                "m3-rule-guard",
            ));
        }
        if needs.definition {
            return Some(needs_more(
                "The question asks for a definition, but no definitional evidence is available.",
                vec!["definition or introductory explanation"],
                NextToolCall {
                    tool: "search_chunks".to_string(),
                    args: serde_json::json!({
                        "query": "definition defined means refers to called proposed introduced",
                        "limit": 8
                    }),
                    reason: "Search for definition-style passages and first explanations."
                        .to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if facts.citation_count == 0 {
            return Some(needs_more(
                "No evidence has been retrieved yet.",
                vec!["document evidence"],
                NextToolCall {
                    tool: "open_pages".to_string(),
                    args: serde_json::json!({ "page": 1, "mode": "overview" }),
                    reason: "Open the document start to obtain grounding evidence.".to_string(),
                },
                "m3-rule-guard",
            ));
        }
        if needs.header {
            return Some(needs_more(
                "The question asks for document header metadata, but no header evidence is available.",
                vec!["title", "authors", "affiliations"],
                NextToolCall {
                    tool: "open_pages".to_string(),
                    args: serde_json::json!({ "page": 1, "mode": "header" }),
                    reason: "Open the first-page header for paper metadata.".to_string(),
                },
                "m3-rule-guard",
            ));
        }
        None
    }
}

#[derive(Default)]
struct HeuristicAnswerabilityJudge;

impl HeuristicAnswerabilityJudge {
    fn decide(&self, needs: &QuestionNeeds, facts: &EvidenceFacts) -> FinalizeDecision {
        if needs.overview && facts.has_overview && facts.evidence_chars >= 280 {
            answerable(
                "The retrieved page overview contains enough title/abstract context.",
                "m3-heuristic-judge",
            )
        } else if needs.overview && !facts.has_overview {
            needs_more(
                "The question asks for a document-level overview, but no page overview evidence is available.",
                vec!["title and abstract"],
                NextToolCall {
                    tool: "open_pages".to_string(),
                    args: serde_json::json!({ "page": 1, "mode": "overview" }),
                    reason: "Open the first-page overview for title and abstract context."
                        .to_string(),
                },
                "m3-heuristic-judge",
            )
        } else if matches!(needs.intent, "explain" | "summarize") && facts.evidence_chars < 360 {
            needs_more(
                "The retrieved evidence is too thin for a grounded explanation.",
                vec!["supporting passage"],
                NextToolCall {
                    tool: "search_chunks".to_string(),
                    args: serde_json::json!({ "query": "broad_context", "page": 1 }),
                    reason: "Broaden retrieval to collect supporting passages.".to_string(),
                },
                "m3-heuristic-judge",
            )
        } else {
            answerable(
                "Retrieved evidence is sufficient for a grounded answer.",
                "m3-heuristic-judge",
            )
        }
    }
}

struct QuestionNeeds<'a> {
    intent: &'a str,
    overview: bool,
    header: bool,
    method: bool,
    experiment: bool,
    figure: bool,
    table: bool,
    definition: bool,
    location: bool,
    reference: bool,
}

impl<'a> QuestionNeeds<'a> {
    fn from_question(question: &str, intent: &'a str) -> Self {
        Self {
            intent,
            overview: asks_document_overview(question, intent),
            header: asks_document_header(question),
            method: asks_method_or_approach(question),
            experiment: asks_experiment_or_result(question),
            figure: asks_figure_or_table(question),
            table: asks_table_metrics(question),
            definition: asks_definition(question),
            location: asks_location(question),
            reference: asks_reference(question),
        }
    }
}

struct EvidenceFacts {
    citation_count: usize,
    evidence_chars: usize,
    has_selection: bool,
    has_overview: bool,
    has_header: bool,
    has_author_evidence: bool,
    has_method: bool,
    has_experiment: bool,
    has_figure: bool,
    has_table: bool,
    has_open_table: bool,
    has_current_view: bool,
    has_reference: bool,
}

impl EvidenceFacts {
    fn from_citations(citations: &[Citation]) -> Self {
        Self {
            citation_count: citations.len(),
            evidence_chars: citations
                .iter()
                .map(|citation| citation.quote.len())
                .sum::<usize>(),
            has_selection: citations
                .iter()
                .any(|citation| citation.source == "selection"),
            has_overview: citations.iter().any(is_page_overview_citation),
            has_header: citations.iter().any(|citation| {
                citation.source == "open_pages"
                    && citation
                        .section_title
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains("header")
            }),
            has_author_evidence: citations.iter().any(|citation| {
                let quote = citation.quote.to_lowercase();
                citation.source == "open_pages"
                    && (quote.contains("author")
                        || quote.contains("authors")
                        || quote.contains("university")
                        || quote.contains("institute")
                        || quote.contains("group"))
            }),
            has_method: has_method_evidence(citations),
            has_experiment: has_experiment_evidence(citations),
            has_figure: has_figure_evidence(citations),
            has_table: has_table_evidence(citations),
            has_open_table: has_open_table_evidence(citations),
            has_current_view: citations
                .iter()
                .any(|citation| citation.source == "current_view"),
            has_reference: has_reference_evidence(citations),
        }
    }
}

fn finish_decision(
    decision: &mut FinalizeDecision,
    citation_count: usize,
    attempt: u32,
    max_attempts: u32,
) {
    decision.citation_count = citation_count;
    decision.attempt = attempt;
    decision.max_attempts = max_attempts;
    if decision.needs_more_evidence && attempt + 1 >= max_attempts {
        decision.status = "insufficient".to_string();
        decision.needs_more_evidence = false;
        decision.budget_exhausted = true;
        decision.next_tool = None;
        decision.next_tool_call = None;
        decision.reason = format!(
            "{} Reached the retrieval step limit.",
            decision.reason.trim()
        );
    }
}

fn answerable(reason: &str, runtime: &str) -> FinalizeDecision {
    FinalizeDecision {
        status: "answerable".to_string(),
        citation_count: 0,
        needs_more_evidence: false,
        reason: reason.to_string(),
        missing: Vec::new(),
        next_tool: None,
        next_tool_call: None,
        attempt: 0,
        max_attempts: 0,
        budget_exhausted: false,
        runtime: runtime.to_string(),
    }
}

fn needs_more(
    reason: &str,
    missing: Vec<&str>,
    next_tool_call: NextToolCall,
    runtime: &str,
) -> FinalizeDecision {
    FinalizeDecision {
        status: "needs_more_evidence".to_string(),
        citation_count: 0,
        needs_more_evidence: true,
        reason: reason.to_string(),
        missing: missing.into_iter().map(str::to_string).collect(),
        next_tool: Some(legacy_next_tool_name(&next_tool_call)),
        next_tool_call: Some(next_tool_call),
        attempt: 0,
        max_attempts: 0,
        budget_exhausted: false,
        runtime: runtime.to_string(),
    }
}

fn legacy_next_tool_name(call: &NextToolCall) -> String {
    if call.tool == "open_pages"
        && call
            .args
            .get("page")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            == 1
    {
        "open_document_start".to_string()
    } else if call.tool == "search_chunks" {
        "search_broad_context".to_string()
    } else {
        call.tool.clone()
    }
}

fn asks_document_header(question: &str) -> bool {
    let normalized = question.to_lowercase();
    normalized.contains("author")
        || normalized.contains("authors")
        || normalized.contains("affiliation")
        || normalized.contains("affiliations")
        || normalized.contains("作者")
        || normalized.contains("署名")
        || normalized.contains("机构")
        || normalized.contains("单位")
        || normalized.contains("标题")
        || normalized.contains("title")
}

fn asks_method_or_approach(question: &str) -> bool {
    let normalized = question.to_lowercase();
    normalized.contains("method")
        || normalized.contains("approach")
        || normalized.contains("methodology")
        || normalized.contains("algorithm")
        || normalized.contains("framework")
        || normalized.contains("principle")
        || normalized.contains("mechanism")
        || normalized.contains("architecture")
        || normalized.contains("方法")
        || normalized.contains("框架")
        || normalized.contains("算法")
        || normalized.contains("原理")
        || normalized.contains("机制")
        || normalized.contains("架构")
        || normalized.contains("怎么做")
        || normalized.contains("如何实现")
}

fn overview_evidence_can_answer(question: &str, needs: &QuestionNeeds<'_>) -> bool {
    if needs.header
        || needs.experiment
        || needs.figure
        || needs.definition
        || needs.location
        || needs.reference
    {
        return false;
    }
    !needs.method || asks_high_level_principle_overview(question)
}

fn asks_high_level_principle_overview(question: &str) -> bool {
    let normalized = question.to_lowercase();
    let asks_principle = normalized.contains("原理")
        || normalized.contains("机制")
        || normalized.contains("principle")
        || normalized.contains("mechanism");
    let asks_specific_design = normalized.contains("具体")
        || normalized.contains("详细")
        || normalized.contains("设计")
        || normalized.contains("算法")
        || normalized.contains("流程")
        || normalized.contains("实现")
        || normalized.contains("architecture")
        || normalized.contains("algorithm")
        || normalized.contains("step")
        || normalized.contains("implementation")
        || normalized.contains("design");
    asks_principle && !asks_specific_design
}

fn asks_experiment_or_result(question: &str) -> bool {
    let normalized = question.to_lowercase();
    normalized.contains("experiment")
        || normalized.contains("evaluation")
        || normalized.contains("result")
        || normalized.contains("benchmark")
        || normalized.contains("performance")
        || normalized.contains("metric")
        || normalized.contains("score")
        || normalized.contains("sota")
        || normalized.contains("实验")
        || normalized.contains("评测")
        || normalized.contains("结果")
        || normalized.contains("指标")
        || normalized.contains("成绩")
        || normalized.contains("分数")
        || normalized.contains("效果")
}

fn asks_table_metrics(question: &str) -> bool {
    let normalized = question.to_lowercase();
    requested_table_number(&normalized).is_some()
        || normalized.contains("table")
        || normalized.contains("benchmark")
        || normalized.contains("metric")
        || normalized.contains("score")
        || normalized.contains("sota")
        || normalized.contains("performance")
        || normalized.contains("表格")
        || normalized.contains("指标")
        || normalized.contains("分数")
        || normalized.contains("成绩")
        || normalized.contains("突出")
        || normalized.contains("领先")
}

fn asks_figure_or_table(question: &str) -> bool {
    let normalized = question.to_lowercase();
    normalized.contains("figure")
        || normalized.contains("table")
        || normalized.contains("caption")
        || normalized.contains("图表")
        || normalized.contains("表格")
        || contains_numbered_cjk_marker(&normalized, '图')
        || contains_numbered_cjk_marker(&normalized, '表')
}

fn asks_definition(question: &str) -> bool {
    let normalized = question.to_lowercase();
    if is_document_overview_definition_collision(&normalized)
        || requested_table_number(&normalized).is_some()
        || normalized.contains("原理")
        || normalized.contains("机制")
        || normalized.contains("principle")
        || normalized.contains("mechanism")
    {
        return false;
    }
    normalized.contains("what is ")
        || normalized.contains("what are ")
        || normalized.contains("what does")
        || normalized.contains("define")
        || normalized.contains("definition")
        || normalized.contains("meaning")
        || normalized.contains("是什么意思")
        || normalized.contains("是什么")
        || normalized.contains("什么是")
        || normalized.contains("指什么")
        || normalized.contains("含义")
        || normalized.contains("定义")
}

fn is_document_overview_definition_collision(normalized: &str) -> bool {
    normalized.contains("what is this paper about")
        || normalized.contains("what is this article about")
        || normalized.contains("这篇文章讲的什么")
        || normalized.contains("这篇文章讲了什么")
        || normalized.contains("这篇文章讲什么")
        || normalized.contains("这篇论文讲的什么")
        || normalized.contains("这篇论文讲了什么")
        || normalized.contains("这篇论文讲什么")
        || normalized.contains("讲的什么")
        || normalized.contains("讲了什么")
        || normalized.contains("讲什么")
        || normalized.contains("关于什么")
}

fn asks_location(question: &str) -> bool {
    let normalized = question.to_lowercase();
    normalized.contains("where")
        || normalized.contains("which page")
        || normalized.contains("what page")
        || normalized.contains("which section")
        || normalized.contains("what section")
        || normalized.contains("located")
        || normalized.contains("location")
        || normalized.contains("appears")
        || normalized.contains("在哪里")
        || normalized.contains("在哪")
        || normalized.contains("哪一页")
        || normalized.contains("第几页")
        || normalized.contains("哪一节")
        || normalized.contains("哪个章节")
        || normalized.contains("位置")
        || normalized.contains("出现在")
}

fn asks_section_location(question: &str) -> bool {
    let normalized = question.to_lowercase();
    normalized.contains("section")
        || normalized.contains("chapter")
        || normalized.contains("哪一节")
        || normalized.contains("哪个章节")
        || normalized.contains("章节")
}

fn asks_reference(question: &str) -> bool {
    let normalized = question.to_lowercase();
    normalized.contains("reference")
        || normalized.contains("references")
        || normalized.contains("citation")
        || normalized.contains("cite")
        || normalized.contains("cited")
        || normalized.contains("bibliography")
        || normalized.contains("related work")
        || normalized.contains("依据")
        || normalized.contains("引用")
        || normalized.contains("参考文献")
        || normalized.contains("出处")
        || normalized.contains("来源")
        || normalized.contains("相关工作")
}

fn contains_numbered_cjk_marker(value: &str, marker: char) -> bool {
    let chars = value.chars().collect::<Vec<_>>();
    chars.iter().enumerate().any(|(index, ch)| {
        if *ch != marker {
            return false;
        }
        chars
            .get(index + 1)
            .copied()
            .filter(|next| {
                next.is_ascii_digit() || matches!(next, '一' | '二' | '三' | '四' | '五')
            })
            .is_some()
            || (chars.get(index + 1) == Some(&' ')
                && chars
                    .get(index + 2)
                    .copied()
                    .filter(|next| {
                        next.is_ascii_digit() || matches!(next, '一' | '二' | '三' | '四' | '五')
                    })
                    .is_some())
    })
}

fn requested_table_number(value: &str) -> Option<String> {
    let normalized = value.to_lowercase();
    for marker in ["table", "表格", "表"] {
        for (index, _) in normalized.match_indices(marker) {
            let rest = &normalized[index + marker.len()..];
            if let Some(number) = leading_reference_number(rest) {
                return Some(number);
            }
        }
    }
    None
}

fn leading_reference_number(value: &str) -> Option<String> {
    let trimmed = value.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, ':' | '#' | '-' | '_' | '.' | '：')
    });
    let digits = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if !digits.is_empty() {
        return digits
            .parse::<u32>()
            .ok()
            .filter(|number| *number > 0)
            .map(|number| number.to_string());
    }
    let cjk = trimmed
        .chars()
        .take_while(|ch| is_cjk_number_char(*ch))
        .collect::<String>();
    if let Some(number) = parse_cjk_number(&cjk) {
        return Some(number.to_string());
    }
    let roman = trimmed
        .chars()
        .take_while(|ch| matches!(ch, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'))
        .collect::<String>();
    if roman.is_empty()
        || trimmed[roman.len()..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphabetic())
    {
        None
    } else {
        roman_to_number(&roman).map(|number| number.to_string())
    }
}

fn is_cjk_number_char(ch: char) -> bool {
    matches!(
        ch,
        '零' | '〇' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
    )
}

fn parse_cjk_number(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    let chars = value.chars().collect::<Vec<_>>();
    if let Some(ten_index) = chars.iter().position(|ch| *ch == '十') {
        let tens = if ten_index == 0 {
            1
        } else {
            cjk_digit(chars[ten_index - 1])?
        };
        let ones = match chars.get(ten_index + 1).copied() {
            Some(ch) => cjk_digit(ch)?,
            None => 0,
        };
        return Some(tens * 10 + ones);
    }
    if chars.len() == 1 {
        return cjk_digit(chars[0]);
    }
    None
}

fn cjk_digit(ch: char) -> Option<u32> {
    match ch {
        '零' | '〇' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

fn roman_to_number(value: &str) -> Option<u32> {
    let mut total = 0_i32;
    let mut previous = 0_i32;
    for ch in value.chars().rev() {
        let current = match ch {
            'i' => 1,
            'v' => 5,
            'x' => 10,
            'l' => 50,
            'c' => 100,
            'd' => 500,
            'm' => 1000,
            _ => return None,
        };
        if current < previous {
            total -= current;
        } else {
            total += current;
            previous = current;
        }
    }
    (total > 0).then_some(total as u32)
}

fn has_method_evidence(citations: &[Citation]) -> bool {
    citations.iter().any(|citation| {
        if is_page_overview_citation(citation) {
            return false;
        }
        citation_matches(
            citation,
            &[
                "method",
                "approach",
                "methodology",
                "algorithm",
                "framework",
            ],
        )
    })
}

fn has_experiment_evidence(citations: &[Citation]) -> bool {
    citations.iter().any(|citation| {
        if is_page_overview_citation(citation) {
            return false;
        }
        if matches!(citation.source.as_str(), "table_fact" | "open_table") {
            return true;
        }
        citation_matches(
            citation,
            &[
                "experiment",
                "evaluation",
                "result",
                "benchmark",
                "performance",
                "metric",
            ],
        )
    })
}

fn has_figure_evidence(citations: &[Citation]) -> bool {
    citations.iter().any(|citation| {
        if matches!(
            citation.source.as_str(),
            "visual_anchor" | "inspect_objects"
        ) {
            return false;
        }
        matches!(
            citation.source.as_str(),
            "visual_asset" | "open_visual" | "analyze_visual" | "analyze_page"
        ) || citation_matches(citation, &["figure", "table", "caption"])
    })
}

fn has_table_evidence(citations: &[Citation]) -> bool {
    citations.iter().any(|citation| {
        matches!(citation.source.as_str(), "table_fact" | "open_table")
            || citation_matches(
                citation,
                &[
                    "table",
                    "benchmark",
                    "metric",
                    "score",
                    "sota",
                    "performance",
                ],
            )
    })
}

fn has_open_table_evidence(citations: &[Citation]) -> bool {
    citations
        .iter()
        .any(|citation| citation.source.as_str() == "open_table")
}

fn has_open_table_evidence_for_number(citations: &[Citation], table_number: &str) -> bool {
    citations
        .iter()
        .filter(|citation| citation.source == "open_table")
        .any(|citation| {
            citation
                .section_title
                .as_deref()
                .and_then(requested_table_number)
                .as_deref()
                == Some(table_number)
                || requested_table_number(&citation.quote).as_deref() == Some(table_number)
        })
}

fn has_current_view_table_evidence_for_number(citations: &[Citation], table_number: &str) -> bool {
    citations
        .iter()
        .filter(|citation| citation.source == "current_view")
        .any(|citation| {
            citation
                .section_title
                .as_deref()
                .and_then(requested_table_number)
                .as_deref()
                == Some(table_number)
                || requested_table_number(&citation.quote).as_deref() == Some(table_number)
        })
}

fn is_page_overview_citation(citation: &Citation) -> bool {
    citation.source == "open_pages"
        && citation
            .section_title
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains("overview")
}

fn has_definition_evidence(question: &str, citations: &[Citation]) -> bool {
    let focus_terms = definition_focus_terms(question);
    citations.iter().any(|citation| {
        let haystack = citation_haystack(citation);
        has_definition_marker(&haystack)
            && (focus_terms.is_empty() || focus_terms.iter().any(|term| haystack.contains(term)))
    })
}

fn has_definition_marker(haystack: &str) -> bool {
    let padded = format!(" {haystack} ");
    [
        " definition ",
        " defined ",
        " refers to ",
        " means ",
        " called ",
        " we propose ",
        " proposed ",
        " introduced ",
        " is a ",
        " is an ",
        " are a ",
        " are an ",
    ]
    .iter()
    .any(|marker| padded.contains(marker))
        || haystack.contains("是一种")
        || haystack.contains("是一个")
        || haystack.contains("指的是")
        || haystack.contains("定义为")
        || haystack.contains("提出")
}

fn definition_focus_terms(question: &str) -> Vec<String> {
    let normalized = question.to_lowercase();
    let mut terms = Vec::new();
    for marker in [
        "是什么意思",
        "是什么",
        "什么是",
        "指什么",
        "what is",
        "what are",
        "what does",
        "define",
        "definition",
        "meaning",
    ] {
        if let Some(index) = normalized.find(marker) {
            push_definition_focus_terms(&normalized[..index], &mut terms);
            push_definition_focus_terms(&normalized[index + marker.len()..], &mut terms);
        }
    }
    push_definition_focus_terms(&normalized, &mut terms);
    terms.dedup();
    terms
}

fn push_definition_focus_terms(value: &str, terms: &mut Vec<String>) {
    let cleaned = value
        .trim_matches(|ch: char| ch.is_whitespace() || matches!(ch, '?' | '？' | ':' | '：'))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.chars().count() > 2 && !is_definition_stop_term(&cleaned) {
        terms.push(cleaned.clone());
    }
    for term in cleaned
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|term| term.chars().count() > 2)
        .filter(|term| !is_definition_stop_term(term))
    {
        terms.push(term.to_string());
    }
}

fn is_definition_stop_term(value: &str) -> bool {
    matches!(
        value,
        "what"
            | "does"
            | "define"
            | "definition"
            | "meaning"
            | "paper"
            | "article"
            | "this"
            | "that"
            | "这个"
            | "这篇"
            | "论文"
            | "文章"
            | "是什么"
            | "是什么意思"
            | "什么是"
            | "指什么"
            | "含义"
            | "定义"
    )
}

fn has_location_evidence(citations: &[Citation], require_section: bool) -> bool {
    citations.iter().any(|citation| {
        if require_section {
            return citation.section_title.as_deref().is_some_and(|title| {
                !title.trim().is_empty() && !title.to_lowercase().contains("overview")
            });
        }
        citation.page > 0
            && (!citation.block_id.trim().is_empty() || !citation.quote.trim().is_empty())
    })
}

fn has_reference_evidence(citations: &[Citation]) -> bool {
    citations.iter().any(|citation| {
        citation_matches(
            citation,
            &[
                "reference",
                "references",
                "citation",
                "cited",
                "related work",
                "bibliography",
                "prior work",
                "参考文献",
                "引用",
                "相关工作",
            ],
        )
    })
}

fn citation_matches(citation: &Citation, needles: &[&str]) -> bool {
    let haystack = citation_haystack(citation);
    needles.iter().any(|needle| haystack.contains(needle))
}

fn citation_haystack(citation: &Citation) -> String {
    format!(
        "{} {}",
        citation.section_title.as_deref().unwrap_or_default(),
        citation.quote
    )
    .to_lowercase()
}

fn asks_document_overview(question: &str, intent: &str) -> bool {
    if intent == "summarize" {
        return true;
    }
    let normalized = question.to_lowercase();
    normalized.contains("what is this paper about")
        || normalized.contains("what is this article about")
        || normalized.contains("summarize this paper")
        || ((normalized.contains("这篇")
            || normalized.contains("文章")
            || normalized.contains("论文"))
            && (normalized.contains("讲的什么")
                || normalized.contains("讲了什么")
                || normalized.contains("讲什么")
                || normalized.contains("讲述了什么")
                || normalized.contains("关于什么")
                || normalized.contains("主要内容")
                || normalized.contains("总结")
                || normalized.contains("概括")
                || normalized.contains("摘要")
                || normalized.contains("重要结论")
                || normalized.contains("主要结论")
                || normalized.contains("核心结论")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn citation(source: &str, section_title: Option<&str>, quote: &str) -> Citation {
        Citation {
            id: "c1".to_string(),
            label: "[1]".to_string(),
            page: 1,
            block_id: "b1".to_string(),
            section_title: section_title.map(str::to_string),
            quote: quote.to_string(),
            bbox_list: serde_json::json!([]),
            document_id: "doc".to_string(),
            source: source.to_string(),
        }
    }

    #[test]
    fn empty_evidence_does_not_exit_as_answerable() {
        let decision = finalize_citations("这篇文章讲的什么？", "explain", &[], 2, 3);
        assert_eq!(decision.status, "insufficient");
        assert!(!decision.needs_more_evidence);
        assert_eq!(decision.next_tool, None);
    }

    #[test]
    fn document_overview_exits_when_page_evidence_is_sufficient() {
        let quote = "MemGPT: Towards LLMs as Operating Systems\n\nAbstract. Large language models are limited by fixed context windows. This paper proposes virtual context management and a memory hierarchy that allows agents to page information between context and external storage while preserving long-running interactions.";
        let decision = finalize_citations(
            "这篇文章讲的什么？",
            "explain",
            &[citation("open_pages", Some("Page 1 overview"), quote)],
            1,
            3,
        );
        assert_eq!(decision.status, "answerable");
        assert!(!decision.needs_more_evidence);
    }

    #[test]
    fn english_document_overview_does_not_route_as_definition() {
        let decision = finalize_citations("What is this paper about?", "explain", &[], 0, 3);

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("overview tool");
        assert_eq!(next_tool.tool, "open_pages");
        assert_eq!(next_tool.args["mode"], serde_json::json!("overview"));
    }

    #[test]
    fn chinese_overview_plus_principle_uses_overview_not_definition() {
        let decision = finalize_citations("这篇文章讲了什么？原理是什么？", "explain", &[], 0, 3);

        assert_eq!(decision.status, "needs_more_evidence");
        assert!(!decision.reason.contains("definition"));
        let next_tool = decision.next_tool_call.expect("overview tool");
        assert_eq!(next_tool.tool, "open_pages");
        assert_eq!(next_tool.args["mode"], serde_json::json!("overview"));
    }

    #[test]
    fn chinese_overview_plus_principle_accepts_abstract_overview() {
        let quote = "GLM-5: from Vibe Coding to Agentic Engineering\n\nAbstract. We present GLM-5, a next-generation foundation model designed to transition the paradigm of vibe coding to agentic engineering. Building upon agentic reasoning and coding capabilities, GLM-5 adopts DSA to reduce training and inference costs while maintaining long-context fidelity, and uses asynchronous reinforcement learning infrastructure to improve post-training efficiency.";
        let decision = finalize_citations(
            "这篇文章讲了什么？原理是什么？",
            "explain",
            &[citation("open_pages", Some("Page 1 overview"), quote)],
            1,
            20,
        );

        assert_eq!(decision.status, "answerable", "decision: {decision:?}");
        assert!(!decision.needs_more_evidence);
        assert_eq!(decision.runtime, "m3-rule-guard");
    }

    #[test]
    fn overview_plus_experiment_does_not_accept_abstract_only() {
        let quote = "GLM-5: from Vibe Coding to Agentic Engineering\n\nAbstract. We present GLM-5, a next-generation foundation model designed to transition the paradigm of vibe coding to agentic engineering. It adopts DSA for long-context fidelity and asynchronous reinforcement learning infrastructure for post-training efficiency.";
        let decision = finalize_citations(
            "这篇文章讲了什么？实验结果怎么样？",
            "explain",
            &[citation("open_pages", Some("Page 1 overview"), quote)],
            1,
            20,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("experiment section tool");
        assert_eq!(next_tool.tool, "open_section");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("experiments evaluation results benchmark metric score SOTA table")
        );
    }

    #[test]
    fn author_question_requests_header_tool_when_missing_metadata() {
        let decision = finalize_citations(
            "这篇论文的作者有哪些？",
            "explain",
            &[citation(
                "fts",
                None,
                "This paper proposes an adaptive context pruning method for coding agents.",
            )],
            0,
            3,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        assert_eq!(
            decision.missing,
            vec![
                "title".to_string(),
                "authors".to_string(),
                "affiliations".to_string()
            ]
        );
        let next_tool = decision.next_tool_call.expect("header tool should be set");
        assert_eq!(next_tool.tool, "open_pages");
        assert_eq!(next_tool.args["page"], serde_json::json!(1));
        assert_eq!(next_tool.args["mode"], serde_json::json!("header"));
    }

    #[test]
    fn author_question_accepts_header_evidence() {
        let decision = finalize_citations(
            "这篇论文的作者有哪些？",
            "explain",
            &[citation(
                "open_pages",
                Some("Page 1 header"),
                "SWE-Pruner\nYuhang Wang, Yuling Shi\nShanghai Jiao Tong University",
            )],
            1,
            3,
        );

        assert_eq!(decision.status, "answerable");
        assert!(decision.next_tool_call.is_none());
    }

    #[test]
    fn method_question_requests_open_section() {
        let decision = finalize_citations(
            "这篇论文的方法框架是什么？",
            "explain",
            &[citation(
                "fts",
                None,
                "This paper studies context pruning for coding agents.",
            )],
            0,
            3,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("method section tool");
        assert_eq!(next_tool.tool, "open_section");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("method approach methodology algorithm framework")
        );
    }

    #[test]
    fn method_and_experiment_question_rejects_page_overview_only() {
        let decision = finalize_citations(
            "这篇文章的方法具体是怎么设计的？请结合方法章节、算法流程和实验结果说明。",
            "explain",
            &[citation(
                "open_pages",
                Some("Page 21 overview"),
                "CASE STUDY ON SWE BENCH. The Pruner-augmented agent applies context pruning to file observations and reports token reduction.",
            )],
            0,
            20,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("method section tool");
        assert_eq!(next_tool.tool, "open_section");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("method approach methodology algorithm framework")
        );
    }

    #[test]
    fn experiment_question_requests_open_section() {
        let decision = finalize_citations(
            "实验结果怎么样？",
            "explain",
            &[citation(
                "fts",
                None,
                "This paper studies context pruning for coding agents.",
            )],
            0,
            3,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("experiment section tool");
        assert_eq!(next_tool.tool, "open_section");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("experiments evaluation results benchmark metric score SOTA table")
        );
    }

    #[test]
    fn table_metric_question_opens_full_table_after_structured_fact_hit() {
        let decision = finalize_citations(
            "请列出 Table 7 中 GLM-5 在 SWE-bench Verified 上的分数。",
            "explain",
            &[citation(
                "table_fact",
                Some("Table 7"),
                "Table 7: Comparison between GLM-5 and open-source/proprietary models. Coding / SWE-bench Verified | GLM-5 = 77.8",
            )],
            1,
            20,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("open table tool");
        assert_eq!(next_tool.tool, "open_table");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("请列出 Table 7 中 GLM-5 在 SWE-bench Verified 上的分数。")
        );
    }

    #[test]
    fn table_explanation_question_opens_requested_table_before_definition_search() {
        let decision = finalize_citations("表 6 是什么，我看不懂，解读一下", "explain", &[], 0, 20);

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("open table tool");
        assert_eq!(next_tool.tool, "open_table");
        assert_eq!(next_tool.args["tableNumber"], serde_json::json!("6"));
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("表 6 是什么，我看不懂，解读一下")
        );
        assert!(!decision.reason.contains("definition"));
    }

    #[test]
    fn table_explanation_question_still_opens_table_after_fact_hit() {
        let decision = finalize_citations(
            "表 6 是什么，我看不懂，解读一下",
            "explain",
            &[citation(
                "table_fact",
                Some("Table 6"),
                "Table 6 | Column 1 | Input Length = 64",
            )],
            1,
            20,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("open table tool");
        assert_eq!(next_tool.tool, "open_table");
        assert_eq!(next_tool.args["tableNumber"], serde_json::json!("6"));
    }

    #[test]
    fn table_explanation_question_accepts_exact_open_table() {
        let decision = finalize_citations(
            "表 6 是什么，我看不懂，解读一下",
            "explain",
            &[citation(
                "open_table",
                Some("Table 6"),
                "Table 6: Average TTFT (ms) for different models and input lengths.",
            )],
            1,
            20,
        );

        assert_eq!(decision.status, "answerable", "decision: {decision:?}");
        assert!(!decision.needs_more_evidence);
    }

    #[test]
    fn table_metric_question_can_finish_after_open_table_evidence() {
        let decision = finalize_citations(
            "请列出 Table 7 中 GLM-5 在 SWE-bench Verified 上的分数。",
            "explain",
            &[citation(
                "open_table",
                Some("Table 7"),
                "Table 7: Comparison between GLM-5 and open-source/proprietary models. Coding / SWE-bench Verified | GLM-5 = 77.8",
            )],
            1,
            20,
        );

        assert_eq!(decision.status, "answerable", "decision: {decision:?}");
        assert!(!decision.needs_more_evidence);
        assert!(decision.reason.contains("LLM evidence judge"));
    }

    #[test]
    fn table_metric_phrase_is_not_treated_as_definition_request() {
        let decision = finalize_citations(
            "Table 3 里面提到的 SWE-Pruner 是什么指标？结果是怎么样的？",
            "explain",
            &[citation(
                "open_table",
                Some("Table 3"),
                "Table 3 | SWE-Pruner | Rounds = 41.1\nTable 3 | SWE-Pruner | Success (%) = 64.0\nTable 3 | SWE-Pruner | Tokens (M) = 0.670",
            )],
            1,
            20,
        );

        assert_eq!(decision.status, "answerable", "decision: {decision:?}");
        assert!(!decision
            .missing
            .iter()
            .any(|missing| missing.contains("definition")));
    }

    #[test]
    fn figure_question_requests_caption_search() {
        let decision = finalize_citations(
            "图 1 说明了什么？",
            "explain",
            &[citation(
                "fts",
                None,
                "This paper studies context pruning for coding agents.",
            )],
            0,
            3,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("figure search tool");
        assert_eq!(next_tool.tool, "inspect_visuals");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("figure table chart caption")
        );
    }

    #[test]
    fn figure_question_does_not_accept_visual_anchor_only() {
        let decision = finalize_citations(
            "Figure 3 说明了什么？",
            "explain",
            &[citation(
                "visual_anchor",
                Some("Figure 3"),
                "Resolved visual anchor: Figure 3 on page 4\nassetId=fig-3",
            )],
            0,
            3,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("visual content tool");
        assert_eq!(next_tool.tool, "inspect_visuals");
    }

    #[test]
    fn definition_question_requests_definition_search() {
        let decision = finalize_citations(
            "SWE-Pruner 是什么？",
            "explain",
            &[citation(
                "fts",
                None,
                "The paper evaluates several coding agents on benchmark tasks.",
            )],
            0,
            3,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("definition search tool");
        assert_eq!(next_tool.tool, "search_chunks");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("definition defined means refers to called proposed introduced")
        );
    }

    #[test]
    fn current_view_evidence_defers_definition_semantics_to_llm() {
        let decision = finalize_citations(
            "这些 Task Type 是什么意思？解读一下",
            "explain",
            &[citation(
                "current_view",
                Some("Current view page evidence: Page 17 lines 2-37"),
                "Table 5 Agentic Tasks Taxonomy used for Query Synthesis\nTask Type Instruction for Query Generation\ncode-summarize Summarize the main purpose or functionality of the code, but do not explain every line.\ncode-refactor Suggest a refactoring or improvement for the code.\nfind-relevant-part Ask to locate or identify the part of the code that implements a specific feature or logic.\ncode-optimize Request an optimization for the code.\ncode-locate Ask to pinpoint the location of a bug, feature, or important logic within the code.\ncode-explain Request an explanation for a particular logic, algorithm, or design choice in the code.",
            )],
            0,
            20,
        );

        assert_eq!(decision.status, "answerable");
        assert!(decision.next_tool_call.is_none());
        assert_eq!(decision.runtime, "m3-rule-guard");
    }

    #[test]
    fn current_view_requested_table_does_not_force_open_table_in_m3() {
        let decision = finalize_citations(
            "Table 3 里面 SWE-Pruner 的数值是什么结果？",
            "explain",
            &[citation(
                "current_view",
                Some("Current view page evidence: Page 8 lines 1-12"),
                "Table 3\nMethod Rounds Success (%) Tokens (M)\nSWE-Pruner 41.1 64.0 0.670",
            )],
            0,
            20,
        );

        assert_eq!(decision.status, "answerable");
        assert!(decision.next_tool_call.is_none());
        assert_eq!(decision.runtime, "m3-rule-guard");
    }

    #[test]
    fn definition_question_accepts_definitional_evidence() {
        let decision = finalize_citations(
            "SWE-Pruner 是什么？",
            "explain",
            &[citation(
                "fts",
                Some("Abstract"),
                "In this paper, we propose SWE-Pruner, a self-adaptive context pruning framework for coding agents.",
            )],
            1,
            3,
        );

        assert_eq!(decision.status, "answerable");
        assert!(decision.next_tool_call.is_none());
    }

    #[test]
    fn definition_question_rejects_irrelevant_chinese_copula_evidence() {
        let decision = finalize_citations(
            "SWE-Pruner 是什么？",
            "explain",
            &[citation(
                "fts",
                Some("Experiments"),
                "这是实验结果，展示模型在多个基准上的性能。",
            )],
            0,
            3,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("definition search tool");
        assert_eq!(next_tool.tool, "search_chunks");
    }

    #[test]
    fn definition_question_accepts_targeted_chinese_definition() {
        let decision = finalize_citations(
            "SWE-Pruner 是什么？",
            "explain",
            &[citation(
                "fts",
                Some("Abstract"),
                "SWE-Pruner 是一种面向 coding agents 的自适应上下文剪枝框架。",
            )],
            1,
            3,
        );

        assert_eq!(decision.status, "answerable");
    }

    #[test]
    fn location_question_requests_section_when_only_plain_chunk_exists() {
        let decision = finalize_citations(
            "这个方法在哪一节介绍？",
            "locate",
            &[citation(
                "fts",
                None,
                "The method uses an adaptive pruning framework.",
            )],
            0,
            3,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("location section tool");
        assert_eq!(next_tool.tool, "open_section");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!(
                "section page location introduced described definition method reference"
            )
        );
    }

    #[test]
    fn location_question_accepts_section_evidence() {
        let decision = finalize_citations(
            "这个方法在哪一节介绍？",
            "locate",
            &[citation(
                "open_section",
                Some("3 Method"),
                "Section: 3 Method\nThe method uses an adaptive pruning framework.",
            )],
            1,
            3,
        );

        assert_eq!(decision.status, "answerable");
    }

    #[test]
    fn reference_question_requests_related_work_or_references_section() {
        let decision = finalize_citations(
            "这句话的引用来源是什么？",
            "explain",
            &[citation(
                "fts",
                None,
                "The method uses an adaptive pruning framework.",
            )],
            0,
            3,
        );

        assert_eq!(decision.status, "needs_more_evidence");
        let next_tool = decision.next_tool_call.expect("reference section tool");
        assert_eq!(next_tool.tool, "open_section");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("references related work citation bibliography source prior work")
        );
    }

    #[test]
    fn reference_question_accepts_reference_evidence() {
        let decision = finalize_citations(
            "相关工作引用了哪些方法？",
            "explain",
            &[citation(
                "open_section",
                Some("2 Related Work"),
                "Section: 2 Related Work\nPrior work on context compression includes LongLLMLingua and LLMLingua.",
            )],
            1,
            3,
        );

        assert_eq!(decision.status, "answerable");
    }

    #[test]
    fn cjk_figure_detection_does_not_match_common_words() {
        let decision = finalize_citations(
            "这个方法的表现如何？",
            "explain",
            &[citation(
                "fts",
                None,
                "This paper studies context pruning for coding agents.",
            )],
            0,
            3,
        );

        let next_tool = decision.next_tool_call.expect("method tool should be set");
        assert_eq!(next_tool.tool, "open_section");
        assert_eq!(
            next_tool.args["query"],
            serde_json::json!("method approach methodology algorithm framework")
        );
    }

    #[test]
    fn table_number_parser_handles_cjk_table_markers() {
        assert_eq!(requested_table_number("表 6 是什么"), Some("6".to_string()));
        assert_eq!(
            requested_table_number("表六解读一下"),
            Some("6".to_string())
        );
        assert_eq!(
            requested_table_number("Table VI latency"),
            Some("6".to_string())
        );
        assert_eq!(requested_table_number("这个方法的表现如何？"), None);
    }
}
