<script lang="ts">
  import { markdown } from '@codemirror/lang-markdown';
  import { EditorState } from '@codemirror/state';
  import { EditorView } from '@codemirror/view';
  import { basicSetup } from 'codemirror';
  import { onMount } from 'svelte';

  let {
    value = $bindable(''),
    placeholder = '写点什么…'
  }: { value?: string; placeholder?: string } = $props();

  let container: HTMLDivElement;
  let view: EditorView | undefined;

  const theme = EditorView.theme({
    '&': {
      minHeight: '55vh',
      fontSize: '15px',
      backgroundColor: 'transparent'
    },
    '.cm-content': { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace' },
    '.cm-scroller': { overflow: 'auto' },
    '&.cm-focused': { outline: 'none' }
  });

  onMount(() => {
    const state = EditorState.create({
      doc: value,
      extensions: [
        basicSetup,
        markdown(),
        EditorView.lineWrapping,
        EditorView.contentAttributes.of({ 'aria-label': placeholder }),
        theme,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            value = update.state.doc.toString();
          }
        })
      ]
    });
    view = new EditorView({ state, parent: container });
    return () => {
      view?.destroy();
      view = undefined;
    };
  });

  $effect(() => {
    if (!view) return;
    const current = view.state.doc.toString();
    if (current !== value) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value }
      });
    }
  });
</script>

<div
  bind:this={container}
  class="min-h-[55vh] rounded-xl border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900"
></div>
