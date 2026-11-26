(function () {
  // builtin: has an engine profile; adding the app enables it
  const STYLE_FOR = { chat: 'casual', email: 'professional', code: 'technical' };
  const app = (id, name, process, kind, icon, builtin) => ({ id, name, process, kind, icon, style: STYLE_FOR[kind] || '', builtin });

  window.AppPresets = [
    app('discord', 'Discord', ['discord'], 'chat', 'discord.svg', 'discord'),
    app('slack', 'Slack', ['slack'], 'chat', 'slack.svg', 'slack'),
    app('teams', 'Microsoft Teams', ['ms-teams', 'teams'], 'chat', 'teams.svg', 'teams'),
    app('whatsapp', 'WhatsApp', ['whatsapp', 'whatsapp.root'], 'chat', 'whatsapp.svg'),
    app('telegram', 'Telegram', ['telegram'], 'chat', 'telegram.svg'),
    app('signal', 'Signal', ['signal'], 'chat', 'signal.svg'),
    app('zoom', 'Zoom', ['zoom'], 'chat', 'zoom.svg'),
    app('skype', 'Skype', ['skype'], 'chat', 'skype.svg'),
    app('mattermost', 'Mattermost', ['mattermost'], 'chat', 'mattermost.svg'),
    app('element', 'Element', ['element'], 'chat', 'element.svg'),

    app('outlook', 'Outlook', ['outlook', 'olk', 'hxoutlook'], 'email', 'outlook.svg', 'outlook'),
    app('thunderbird', 'Thunderbird', ['thunderbird'], 'email', 'thunderbird.svg'),
    app('protonmail', 'Proton Mail', ['protonmail', 'proton mail'], 'email', 'protonmail.svg'),

    app('chrome', 'Google Chrome', ['chrome'], 'auto', 'chrome.svg'),
    app('edge', 'Microsoft Edge', ['msedge'], 'auto', 'edge.svg'),
    app('firefox', 'Firefox', ['firefox'], 'auto', 'firefox.svg'),
    app('brave', 'Brave', ['brave'], 'auto', 'brave.svg'),
    app('opera', 'Opera', ['opera'], 'auto', 'opera.svg'),
    app('vivaldi', 'Vivaldi', ['vivaldi'], 'auto', 'vivaldi.svg'),
    app('arc', 'Arc', ['arc'], 'auto', 'arc.svg'),

    app('vscode', 'Visual Studio Code', ['code'], 'code', 'vscode.svg', 'vscode'),
    app('cursor', 'Cursor', ['cursor'], 'code', 'cursor.svg', 'cursor'),
    app('windsurf', 'Windsurf', ['windsurf'], 'code', 'windsurf.svg', 'windsurf'),
    app('visualstudio', 'Visual Studio', ['devenv'], 'code', 'visualstudio.svg', 'visualstudio'),
    app('jetbrains', 'JetBrains IDEs', ['idea64', 'pycharm64', 'webstorm64', 'rider64', 'goland64', 'clion64'], 'code', 'jetbrains.svg', 'jetbrains'),
    app('androidstudio', 'Android Studio', ['studio64'], 'code', 'androidstudio.svg'),
    app('sublime', 'Sublime Text', ['sublime_text'], 'code', 'sublime.svg'),
    app('notepadpp', 'Notepad++', ['notepad++'], 'code', 'notepadpp.svg'),
    app('zed', 'Zed', ['zed'], 'code', 'zed.svg'),
    app('githubdesktop', 'GitHub Desktop', ['githubdesktop'], 'plain', 'githubdesktop.svg'),

    app('word', 'Microsoft Word', ['winword'], 'document', 'word.svg', 'word'),
    app('excel', 'Microsoft Excel', ['excel'], 'plain', 'excel.svg'),
    app('powerpoint', 'Microsoft PowerPoint', ['powerpnt'], 'document', 'powerpoint.svg'),
    app('notion', 'Notion', ['notion'], 'document', 'notion.svg', 'notion'),
    app('obsidian', 'Obsidian', ['obsidian'], 'document', 'obsidian.svg'),
    app('notepad', 'Notepad', ['notepad'], 'plain', 'notepad.svg', 'notepad'),
    app('onenote', 'OneNote', ['onenote', 'onenoteim'], 'document', 'onenote.svg'),
    app('evernote', 'Evernote', ['evernote'], 'document', 'evernote.svg'),
    app('joplin', 'Joplin', ['joplin'], 'document', 'joplin.svg'),

    app('claude', 'Claude', ['claude'], 'prompt', 'claude.svg'),
    app('chatgpt', 'ChatGPT', ['chatgpt'], 'prompt', 'chatgpt.svg'),
    app('perplexity', 'Perplexity', ['perplexity'], 'prompt', 'perplexity.svg'),
    app('lmstudio', 'LM Studio', ['lm studio'], 'prompt', 'lmstudio.svg'),
    app('ollama', 'Ollama', ['ollama app', 'ollama'], 'prompt', 'ollama.svg'),

    app('figma', 'Figma', ['figma'], 'auto', 'figma.svg'),
    app('linear', 'Linear', ['linear'], 'auto', 'linear.svg'),
    app('postman', 'Postman', ['postman'], 'code', 'postman.svg'),
    app('todoist', 'Todoist', ['todoist'], 'plain', 'todoist.svg'),
    app('miro', 'Miro', ['miro'], 'auto', 'miro.svg'),
  ];
})();
