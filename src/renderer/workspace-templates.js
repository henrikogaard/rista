// ── Starter Workspace Templates (Task 5.1) ──────────────────────
// Creates folder structures for common note-taking methodologies.
// Each template creates folders and optionally a seed file.

export const WORKSPACE_TEMPLATES = {
  // Writing-first: just a clean folder with a daily notes subfolder
  'plain': {
    name: 'Plain Notes',
    description: 'A clean folder with a daily notes subfolder',
    folders: ['daily'],
    files: [],
  },
  // PARA: Projects, Areas, Resources, Archive
  'para': {
    name: 'PARA',
    description: 'Projects, Areas, Resources, Archive — Tiago Forte method',
    folders: ['Projects', 'Areas', 'Resources', 'Archive'],
    files: [
      {
        path: 'Projects/untitled.md',
        content: '# Project\n\n## Objective\n\n\n\n## Key Results\n\n- [ ] \n\n## Notes\n\n',
      },
      {
        path: 'Areas/untitled.md',
        content: '# Area\n\n## Responsibilities\n\n\n\n## Current status\n\n',
      },
      {
        path: 'Resources/untitled.md',
        content: '# Resource\n\n## Source\n\n\n\n## Key takeaways\n\n',
      },
    ],
  },
  // GTD: Inbox, Next Actions, Waiting, Someday, Reference, Projects
  'gtd': {
    name: 'GTD',
    description: 'Getting Things Done — David Allen method',
    folders: ['Inbox', 'Next Actions', 'Waiting', 'Someday', 'Reference', 'Projects'],
    files: [
      {
        path: 'Inbox/untitled.md',
        content: '# Inbox\n\nCapture everything here. Process daily.\n\n- [ ] \n',
      },
      {
        path: 'Next Actions/untitled.md',
        content: '# Next Actions\n\nSingle next physical action for each active item.\n\n- [ ] \n',
      },
      {
        path: 'Waiting/untitled.md',
        content: '# Waiting\n\nThings waiting for someone else.\n\n- [ ] \n',
      },
    ],
  },
  // Zettelkasten: fleeting, literature, permanent + index
  'zettelkasten': {
    name: 'Zettelkasten',
    description: 'Slip-box method — fleeting, literature, permanent notes',
    folders: ['Fleeting', 'Literature', 'Permanent'],
    files: [
      {
        path: 'index.md',
        content: '# Zettelkasten Index\n\n## Structure\n\n- **Fleeting/**: quick capture, daily scratch\n- **Literature/**: notes on what you read\n- **Permanent/**: evergreen atomic notes\n\n## Links\n\n',
      },
    ],
  },
}

export async function applyWorkspaceTemplate(folderPath, templateId) {
  const tmpl = WORKSPACE_TEMPLATES[templateId]
  if (!tmpl) return { ok: false, error: `Unknown template: ${templateId}` }

  try {
    // Create folders
    for (const folder of tmpl.folders) {
      const dirPath = `${folderPath}/${folder}`
      const ok = await window.fjord.createDir(dirPath)
      if (!ok) return { ok: false, error: `Failed to create folder: ${folder}` }
    }

    // Create seed files
    for (const file of tmpl.files) {
      const filePath = `${folderPath}/${file.path}`
      await window.fjord.createFile(filePath)
      await window.fjord.writeFile(filePath, file.content)
    }

    return { ok: true }
  } catch (err) {
    return { ok: false, error: err.message }
  }
}
