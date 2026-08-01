// ── Panel Registration ───────────────────────────────────────────
// All right/left sidebar panel registrations live here.
// Called once at boot with all needed callbacks injected.

import { registerRightPanel, closeRightPanel } from './right-panel.js'
import { buildFileExplorerPanel, mountFileExplorerPanel, refreshFileExplorerState, registerFileExplorerCallbacks, fileExplorerHeaderActions } from './file-explorer-view.js'
import { buildAgentsPanel, mountAgentsPanel, refreshAgentsPanel, registerAgentsViewCallbacks, agentsViewHeaderActions } from './agents-view.js'
import { buildOutlinePanel, mountOutlinePanel, renderOutline } from './outline-view.js'
import { buildTagsPanel, mountTagsPanel, renderTagsPanel, handleTagsPanelEvent } from './tags-view.js'
import { buildWikiQualityPanel, mountWikiQualityPanel, renderWikiQualityPanel, handleWikiQualityPanelEvent } from './wiki-quality-view.js'
import { buildRelatedNotesPanel, mountRelatedNotesPanel, renderRelatedNotesPanel, handleRelatedNotesPanelEvent } from './related-notes-view.js'
import { buildBookmarksPanel, mountBookmarksPanel, unmountBookmarksPanel, renderBookmarks, setBookmarksOpenFile } from './bookmarks-view.js'
import { buildPropertiesPanel, mountPropertiesPanel, renderProperties } from './properties-view.js'
import { buildCalendarPanel, refreshCalendarPanel } from './calendar-view.js'
import { buildGraphView, renderGraph, destroyGraph, setGraphLocalMode, getGraphLocalMode } from './graph-view.js'
import { getLinkIndex, onLinkIndexChange } from './link-index.js'
import { graphIcon, calendarIcon, outlineIcon, bookmarkIcon, propertiesIcon, folderIcon, agentsIcon, tagIcon, wikiQualityIcon, relatedNotesIcon } from './icons.js'
import { initInspectorPanel } from './inspector.js'
import { initAiChatPanel } from './ai-chat.js'
import { state } from './state.js'
import { getFocusedTab } from './state.js'
import { getSettings } from './settings.js'

function _featureEnabled(key) { return getSettings()[key] || getSettings().showExperimental }

export function initPanels({ openFile, refreshTree, openAiSession, openAiChatSurface, createDailyNote, collapseAllFolders, openFolderPath }) {
  const rerenderGraphFromIndex = () => {
    setGraphLocalMode(getGraphLocalMode(), getFocusedTab()?.path || null)
    renderGraph(getLinkIndex(), (path) => openFile({ path, name: path.split('/').pop() }))
  }

  // ── File explorer (left sidebar, always active) ──────────────
  registerFileExplorerCallbacks({
    openFolder,
    collapseAllFolders,
    renderTree: () => { refreshTree() },
  })
  registerRightPanel('files', {
    title: 'Files',
    icon: folderIcon(),
    flex: 3,
    defaultSide: 'left',
    defaultActive: true,
    build: buildFileExplorerPanel,
    headerActions: fileExplorerHeaderActions,
    onMount: mountFileExplorerPanel,
    onUnmount: () => {},
    onRefresh: refreshFileExplorerState,
  })

  // ── Agents (left sidebar, gated) ─────────────────────────────
  registerAgentsViewCallbacks({
    createSession: async () => {
      const { createSession } = await import('./agents-sidebar.js')
      const session = await createSession()
      if (session) refreshAgentsPanel()
    },
    openSession: (sessionPath) => {
      openAiChatSurface()
      openAiSession(sessionPath)
    },
  })
  _featureEnabled('featureAgents') && registerRightPanel('agents', {
    title: 'Agents',
    icon: agentsIcon(),
    flex: 1,
    defaultSide: 'left',
    build: buildAgentsPanel,
    headerActions: agentsViewHeaderActions,
    onMount: mountAgentsPanel,
    onUnmount: () => {},
    onRefresh: refreshAgentsPanel,
  })

  // ── Inspector (gated) ────────────────────────────────────────
  _featureEnabled('featureInspector') && initInspectorPanel(openFile, closeRightPanel)

  // ── Graph (gated) ────────────────────────────────────────────
  let _graphUnsubscribe = null
  _featureEnabled('featureGraphView') && registerRightPanel('graph', {
    title: 'Graph',
    icon: graphIcon(),
    flex: 2,
    build: () => `<div id="graph-panel-body" class="widget-fill">${buildGraphView()}</div>`,
    onMount: () => {
      rerenderGraphFromIndex()
      _graphUnsubscribe?.()
      _graphUnsubscribe = onLinkIndexChange(() => rerenderGraphFromIndex())
    },
    onUnmount: () => {
      _graphUnsubscribe?.()
      _graphUnsubscribe = null
      destroyGraph()
    },
    onRefresh: () => {
      if (getGraphLocalMode()) {
        setGraphLocalMode(true, getFocusedTab()?.path || null)
      }
    },
  })

  // ── AI chat (gated) ──────────────────────────────────────────
  _featureEnabled('featureAgents') && initAiChatPanel(openFile, closeRightPanel)

  // ── Properties (gated) ───────────────────────────────────────
  _featureEnabled('featureProperties') && registerRightPanel('properties', {
    title: 'Properties',
    icon: propertiesIcon(),
    flex: 1,
    defaultSide: 'left',
    defaultActive: true,
    build: buildPropertiesPanel,
    onMount: mountPropertiesPanel,
    onUnmount: () => {},
    onRefresh: renderProperties,
  })

  // ── Outline (always active) ──────────────────────────────────
  registerRightPanel('outline', {
    title: 'Outline',
    icon: outlineIcon(),
    flex: 1,
    build: buildOutlinePanel,
    onMount: mountOutlinePanel,
    onUnmount: () => {},
    onRefresh: renderOutline,
  })

  // ── Tags (gated) ─────────────────────────────────────────────
  let _tagsUnsubscribe = null
  _featureEnabled('featureTags') && registerRightPanel('tags', {
    title: 'Tags',
    icon: tagIcon(),
    flex: 1,
    defaultSide: 'left',
    build: buildTagsPanel,
    onMount: () => {
      mountTagsPanel(openFile, (tag) => {
        state.tagFilter = tag
        refreshTree()
      }, () => {
        state.tagFilter = null
        refreshTree()
      })
      const body = document.getElementById('tags-view-body')
      body?.addEventListener('click', handleTagsPanelEvent)
      body?.addEventListener('keydown', handleTagsPanelEvent)
      _tagsUnsubscribe?.()
      _tagsUnsubscribe = onLinkIndexChange(() => renderTagsPanel())
    },
    onUnmount: () => {
      const body = document.getElementById('tags-view-body')
      body?.removeEventListener('click', handleTagsPanelEvent)
      body?.removeEventListener('keydown', handleTagsPanelEvent)
      _tagsUnsubscribe?.()
      _tagsUnsubscribe = null
    },
    onRefresh: renderTagsPanel,
  })

  // ── Related Notes (gated) ────────────────────────────────────
  let _relatedNotesUnsubscribe = null
  _featureEnabled('featureRelatedNotes') && registerRightPanel('related-notes', {
    title: 'Related',
    icon: relatedNotesIcon(),
    flex: 1,
    build: buildRelatedNotesPanel,
    onMount: () => {
      mountRelatedNotesPanel(openFile)
      const body = document.getElementById('related-notes-body')
      body?.addEventListener('click', handleRelatedNotesPanelEvent)
      body?.addEventListener('keydown', handleRelatedNotesPanelEvent)
      _relatedNotesUnsubscribe?.()
      _relatedNotesUnsubscribe = onLinkIndexChange(() => renderRelatedNotesPanel())
    },
    onUnmount: () => {
      const body = document.getElementById('related-notes-body')
      body?.removeEventListener('click', handleRelatedNotesPanelEvent)
      body?.removeEventListener('keydown', handleRelatedNotesPanelEvent)
      _relatedNotesUnsubscribe?.()
      _relatedNotesUnsubscribe = null
    },
    onRefresh: renderRelatedNotesPanel,
  })

  // ── Wiki Quality (gated) ─────────────────────────────────────
  let _wikiQualityUnsubscribe = null
  _featureEnabled('featureWikiQuality') && registerRightPanel('wiki-quality', {
    title: 'Wiki',
    icon: wikiQualityIcon(),
    flex: 1,
    build: buildWikiQualityPanel,
    onMount: () => {
      mountWikiQualityPanel(openFile, { refreshTree })
      const body = document.getElementById('wiki-quality-body')
      body?.addEventListener('click', handleWikiQualityPanelEvent)
      body?.addEventListener('keydown', handleWikiQualityPanelEvent)
      _wikiQualityUnsubscribe?.()
      _wikiQualityUnsubscribe = onLinkIndexChange(() => renderWikiQualityPanel())
    },
    onUnmount: () => {
      const body = document.getElementById('wiki-quality-body')
      body?.removeEventListener('click', handleWikiQualityPanelEvent)
      body?.removeEventListener('keydown', handleWikiQualityPanelEvent)
      _wikiQualityUnsubscribe?.()
      _wikiQualityUnsubscribe = null
    },
    onRefresh: renderWikiQualityPanel,
  })

  // ── Bookmarks (gated) ────────────────────────────────────────
  _featureEnabled('featureBookmarks') && setBookmarksOpenFile((item) => openFile(item))
  _featureEnabled('featureBookmarks') && registerRightPanel('bookmarks', {
    title: 'Bookmarks',
    icon: bookmarkIcon(),
    flex: 1,
    build: buildBookmarksPanel,
    onMount: mountBookmarksPanel,
    onUnmount: unmountBookmarksPanel,
    onRefresh: renderBookmarks,
  })

  // ── Calendar (gated) ─────────────────────────────────────────
  _featureEnabled('featureCalendar') && registerRightPanel('calendar', {
    title: 'Calendar',
    icon: calendarIcon(),
    flex: 0,
    build: () => `<div id="calendar-panel-body" class="widget-fill"></div>`,
    onMount: () => {
      const body = document.getElementById('calendar-panel-body')
      if (body && !body.firstChild) {
        const panel = buildCalendarPanel({ onDateClick: (dateStr) => createDailyNote(dateStr), folderPath: state.folderPath })
        body.appendChild(panel)
      }
    },
    onUnmount: () => { document.getElementById('calendar-panel-body')?.replaceChildren() },
  })
}
