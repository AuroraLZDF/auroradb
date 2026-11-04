import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  drawSelection,
  dropCursor,
  rectangularSelection,
  crosshairCursor,
  highlightSpecialChars,
  Decoration,
  placeholder,
  type DecorationSet,
} from '@codemirror/view'
import { EditorState, StateEffect, StateField, Compartment, type Extension } from '@codemirror/state'
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
} from '@codemirror/commands'
import {
  foldGutter,
  indentOnInput,
  bracketMatching,
  syntaxHighlighting,
  HighlightStyle,
} from '@codemirror/language'
import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
  type CompletionContext,
  type CompletionResult,
} from '@codemirror/autocomplete'
import { search, searchKeymap, highlightSelectionMatches } from '@codemirror/search'
import { sql as sqlLanguage, MySQL, PostgreSQL } from '@codemirror/lang-sql'
import { tags as t } from '@lezer/highlight'
import type { CompletionItem, CompletionResult as EngineResult } from '@/composables/useSqlCompletion'
import type { DriverKind } from '@/stores/savedConnections'

/** 错误行高亮的状态效果：参数为 1 起始的行号，null 表示清除 */
const setErrorLineEffect = StateEffect.define<number | null>()

const errorLineField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(decorations, tr) {
    let next = decorations.map(tr.changes)
    for (const effect of tr.effects) {
      if (effect.is(setErrorLineEffect)) {
        if (effect.value === null) {
          next = Decoration.none
        } else {
          const line = tr.state.doc.line(effect.value)
          next = Decoration.set([
            Decoration.line({ class: 'cm-errorLine' }).range(line.from),
          ])
        }
      }
    }
    return next
  },
  provide: field => EditorView.decorations.from(field),
})

/** 语法高亮配色：使用主题 CSS 变量，亮暗主题自动适配 */
const sqlHighlightStyle = HighlightStyle.define([
  { tag: t.keyword, color: 'var(--syntax-keyword, #7c3aed)' },
  { tag: [t.string, t.special(t.string)], color: 'var(--syntax-string, #16a34a)' },
  { tag: [t.number, t.bool, t.null], color: 'var(--syntax-number, #ea580c)' },
  { tag: [t.comment, t.lineComment, t.blockComment], color: 'var(--color-text-secondary)', fontStyle: 'italic' },
  { tag: t.operator, color: 'var(--syntax-operator, #0891b2)' },
  { tag: [t.punctuation, t.separator], color: 'var(--color-text)' },
  { tag: [t.variableName, t.propertyName], color: 'var(--color-text)' },
  { tag: [t.typeName, t.function(t.variableName)], color: 'var(--syntax-function, #2563eb)' },
])

export interface SqlEditorApi {
  view: EditorView
  /** 用新文本整体替换文档（格式化 / 加载历史 / 清空） */
  replaceDoc(text: string): void
  /** 跳转并高亮 1 起始的行号；传 null 清除高亮 */
  highlightErrorLine(line: number | null): void
  /** 切换 SQL 方言（连接驱动变化时调用） */
  setDriver(driver: DriverKind): void
  focus(): void
  destroy(): void
}

export interface CreateSqlEditorOptions {
  /** 挂载容器元素 */
  parent: HTMLElement
  /** 初始 SQL 文本（来自 editorTabs store） */
  initialValue: string
  /** 数据库驱动，决定 SQL 方言 */
  driver: DriverKind
  placeholder: string
  /** 文本变化回调（含用户输入与程序替换） */
  onChange: (value: string) => void
  /** F5 触发执行 */
  onExecute: () => void
  /** Shift+Alt+F 触发格式化 */
  onFormat: () => void
  /** 补全引擎适配：返回 null 表示不弹补全 */
  complete: (sql: string, caret: number, explicit: boolean) => EngineResult | null
}

/** 把引擎补全结果适配成 CM6 autocomplete source */
function makeCompletionSource(
  options: CreateSqlEditorOptions
): (context: CompletionContext) => Promise<CompletionResult | null> | CompletionResult | null {
  return (context: CompletionContext) => {
    const doc = context.state.doc
    const caret = context.pos
    if (caret > doc.length) return null

    const result = options.complete(doc.toString(), caret, context.explicit)
    if (!result || result.items.length === 0) return null

    return {
      from: result.replaceStart,
      to: result.replaceEnd,
      options: result.items.map((item: CompletionItem) => ({
        label: item.label,
        detail: item.detail,
        type: item.kind,
        boost: item.kind === 'column' ? 1 : item.kind === 'keyword' ? -1 : 0,
      })),
      // 输入纯标识符字符时复用上次结果，避免每次按键重算
      validFor: /^[A-Za-z0-9_$.`]*$/,
    }
  }
}

/** 主题：跟随应用的亮暗 CSS 变量 */
function editorTheme(): Extension {
  return EditorView.theme(
    {
      '&': {
        height: '100%',
        fontSize: '14px',
        color: 'var(--color-text)',
        backgroundColor: 'var(--color-background)',
      },
      '&.cm-focused': {
        outline: 'none',
      },
      '&.cm-focused .cm-selectionBackground, ::selection': {
        backgroundColor: 'var(--color-primary-soft, rgba(59, 130, 246, 0.25))',
      },
      '.cm-scroller': {
        overflow: 'auto',
        fontFamily: "'Consolas', 'Monaco', 'Courier New', monospace",
        lineHeight: '1.5',
      },
      '.cm-content': {
        caretColor: 'var(--color-text)',
        paddingBottom: '16px',
      },
      '.cm-cursor': {
        borderLeftColor: 'var(--color-text)',
      },
      '.cm-gutters': {
        backgroundColor: 'var(--color-background)',
        color: 'var(--color-text-secondary)',
        border: 'none',
        borderRight: '1px solid var(--color-border)',
      },
      '.cm-activeLine': {
        backgroundColor: 'var(--color-background-soft)',
      },
      '.cm-activeLineGutter': {
        backgroundColor: 'var(--color-background-soft)',
        color: 'var(--color-text)',
      },
      '.cm-selectionMatch': {
        backgroundColor: 'var(--color-primary-soft, rgba(59, 130, 246, 0.2))',
      },
      '.cm-tooltip.cm-tooltip-autocomplete > ul': {
        fontFamily: "'Consolas', 'Monaco', 'Courier New', monospace",
        maxHeight: '220px',
      },
      '.cm-tooltip.cm-tooltip-autocomplete > ul > li': {
        padding: '3px 8px',
        fontSize: '12px',
      },
      '.cm-errorLine': {
        backgroundColor: 'var(--danger-soft, rgba(244, 67, 54, 0.12))',
      },
    },
    { dark: false }
  )
}

/**
 * 创建 CodeMirror 6 SQL 编辑器实例。
 *
 * 能力：行号、SQL 语法高亮（按驱动选方言）、schema 感知补全（复用 useSqlCompletion
 * 引擎）、括号/引号配对、代码折叠、查找替换（Ctrl+F）、多光标、历史（Ctrl+Z）；
 * F5 执行、Shift+Alt+F 格式化、Ctrl+Space 手动补全。
 */
export function createSqlEditor(options: CreateSqlEditorOptions): SqlEditorApi {
  const languageCompartment = new Compartment()

  const completionSource = makeCompletionSource(options)

  const languageExtension = sqlLanguage({
    dialect: options.driver === 'postgres' ? PostgreSQL : MySQL,
  })

  const updateListener = EditorView.updateListener.of(update => {
    if (update.docChanged) {
      options.onChange(update.state.doc.toString())
    }
  })

  const state = EditorState.create({
    doc: options.initialValue,
    extensions: [
      lineNumbers(),
      highlightActiveLine(),
      highlightSpecialChars(),
      history(),
      drawSelection(),
      dropCursor(),
      rectangularSelection(),
      crosshairCursor(),
      foldGutter(),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      highlightSelectionMatches(),
      search({ top: true }),
      errorLineField,
      languageCompartment.of(languageExtension),
      syntaxHighlighting(sqlHighlightStyle, { fallback: true }),
      autocompletion({
        override: [completionSource],
        icons: false,
        activateOnTyping: true,
        maxRenderedOptions: 50,
      }),
      placeholder(options.placeholder),
      editorTheme(),
      updateListener,
      keymap.of([
        // 自定义快捷键放在默认键位之前，保证优先命中
        { key: 'F5', preventDefault: true, run: () => (options.onExecute(), true) },
        {
          key: 'Shift-Alt-f',
          preventDefault: true,
          run: () => (options.onFormat(), true),
        },
        ...closeBracketsKeymap,
        ...defaultKeymap,
        ...searchKeymap,
        ...historyKeymap,
        ...completionKeymap,
        indentWithTab,
      ]),
    ],
  })

  const view = new EditorView({ state, parent: options.parent })

  return {
    view,
    replaceDoc(text: string) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
      })
    },
    highlightErrorLine(line: number | null) {
      view.dispatch({ effects: setErrorLineEffect.of(line) })
      if (line !== null) {
        const target = view.state.doc.line(Math.min(line, view.state.doc.lines)).from
        view.dispatch({
          effects: EditorView.scrollIntoView(target, { y: 'center' }),
        })
      }
    },
    setDriver(driver: DriverKind) {
      view.dispatch({
        effects: languageCompartment.reconfigure(
          sqlLanguage({ dialect: driver === 'postgres' ? PostgreSQL : MySQL })
        ),
      })
    },
    focus() {
      view.focus()
    },
    destroy() {
      view.destroy()
    },
  }
}
