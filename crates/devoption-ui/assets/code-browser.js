/* Progressive enhancement only: source and Markdown remain usable without JS. */
(() => {
  'use strict';
  const languages = {
    rs: 'rust', js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'javascript',
    ts: 'typescript', tsx: 'typescript', json: 'json', toml: 'ini', ini: 'ini',
    py: 'python', rb: 'ruby', go: 'go', java: 'java', cs: 'csharp', c: 'c', h: 'c',
    cpp: 'cpp', hpp: 'cpp', css: 'css', scss: 'scss', html: 'xml', xml: 'xml', svg: 'xml',
    sh: 'bash', bash: 'bash', yml: 'yaml', yaml: 'yaml', md: 'markdown', markdown: 'markdown',
    sql: 'sql', php: 'php', swift: 'swift', kt: 'kotlin', diff: 'diff', patch: 'diff'
  };
  document.querySelectorAll('[data-source-path], .markdown-preview pre code').forEach(code => {
    const path = code.dataset.sourcePath;
    const language = path
      ? languages[path.split('.').pop().toLowerCase()]
      : Array.from(code.classList).find(name => name.startsWith('language-'))?.slice(9);
    if (!language || !hljs.getLanguage(language)) return;
    // Never guess a language or pass repository text to an HTML parser directly.
    code.className = `language-${language}`;
    hljs.highlightElement(code);
  });
})();
