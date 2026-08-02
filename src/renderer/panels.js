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
import { buildCalendarPanel } from './calendar-view.js'
import { buildGraphView, renderGraph, destroyGraph, setGraphLocalMode, getGraphLocalMode } from './graph-view.js'
import { getLinkIndex, onLinkIndexChange } from './link-index.js'
import { graphIcon, calendarIcon, outlineIcon, bookmarkIcon, propertiesIcon, folderIcon, agentsIcon, tagIcon, wikiQualityIcon, relatedNotesIcon } from './icons.js'
import { initInspectorPanel } from './inspector.js'
import { initAiChatPanel } from './ai-chat.js'
import { state, $, getFocusedTab, fileName } from './state.js'

export function initPanels({ openFile, refreshTree, openAiSession, openAiChatSurface, createDailyNote, collapseAllFolders, openFolder }) {
  const rerenderGraphFromIndex = () => {
    setGraphLocalMode(getGraphLocalMode(), getFocusedTab()?.path || null)
    renderGraph(getLinkIndex(), (path) => openFile({ path, name: fileName(path) }))
  }

  // ── File explorer (left sidebar, always active) ──────────────
  registerFileExplorerCallbacks({
    openFolder,
    collapseAllFolders,
    renderTree: () => { refreshTree() },
  })
  registerRightPanel('files', {
    title: 'Notes',
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
  registerRightPanel('agents', {
    title: 'Agents',
    feature: 'featureAgents',
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
  initInspectorPanel(openFile, closeRightPanel)

  // ── Graph (gated) ────────────────────────────────────────────
  let _graphUnsubscribe = null
  registerRightPanel('graph', {
    title: 'Graph',
    feature: 'featureGraphView',
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
  initAiChatPanel(openFile, closeRightPanel)

  // ── Properties (gated) ───────────────────────────────────────
  registerRightPanel('properties', {
    title: 'Properties',
    feature: 'featureProperties',
    icon: propertiesIcon(),
    flex: 1,
    group: 'context',
    defaultSide: 'right',
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
    group: 'context',
    defaultActive: true,
    build: buildOutlinePanel,
    onMount: mountOutlinePanel,
    onUnmount: () => {},
    onRefresh: renderOutline,
  })

  // ── Tags (gated) ─────────────────────────────────────────────
  let _tagsUnsubscribe = null
  registerRightPanel('tags', {
    title: 'Tags',
    feature: 'featureTags',
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
      const body = $('tags-view-body')
      body?.addEventListener('click', handleTagsPanelEvent)
      body?.addEventListener('keydown', handleTagsPanelEvent)
      _tagsUnsubscribe?.()
      _tagsUnsubscribe = onLinkIndexChange(() => renderTagsPanel())
    },
    onUnmount: () => {
      const body = $('tags-view-body')
      body?.removeEventListener('click', handleTagsPanelEvent)
      body?.removeEventListener('keydown', handleTagsPanelEvent)
      _tagsUnsubscribe?.()
      _tagsUnsubscribe = null
    },
    onRefresh: renderTagsPanel,
  })

  // ── Related Notes (gated) ────────────────────────────────────
  let _relatedNotesUnsubscribe = null
  registerRightPanel('related-notes', {
    title: 'Related',
    feature: 'featureRelatedNotes',
    icon: relatedNotesIcon(),
    flex: 1,
    build: buildRelatedNotesPanel,
    onMount: () => {
      mountRelatedNotesPanel(openFile)
      const body = $('related-notes-body')
      body?.addEventListener('click', handleRelatedNotesPanelEvent)
      body?.addEventListener('keydown', handleRelatedNotesPanelEvent)
      _relatedNotesUnsubscribe?.()
      _relatedNotesUnsubscribe = onLinkIndexChange(() => renderRelatedNotesPanel())
    },
    onUnmount: () => {
      const body = $('related-notes-body')
      body?.removeEventListener('click', handleRelatedNotesPanelEvent)
      body?.removeEventListener('keydown', handleRelatedNotesPanelEvent)
      _relatedNotesUnsubscribe?.()
      _relatedNotesUnsubscribe = null
    },
    onRefresh: renderRelatedNotesPanel,
  })

  // ── Wiki Quality (gated) ─────────────────────────────────────
  let _wikiQualityUnsubscribe = null
  registerRightPanel('wiki-quality', {
    title: 'Wiki',
    feature: 'featureWikiQuality',
    icon: wikiQualityIcon(),
    flex: 1,
    build: buildWikiQualityPanel,
    onMount: () => {
      mountWikiQualityPanel(openFile, { refreshTree })
      const body = $('wiki-quality-body')
      body?.addEventListener('click', handleWikiQualityPanelEvent)
      body?.addEventListener('keydown', handleWikiQualityPanelEvent)
      _wikiQualityUnsubscribe?.()
      _wikiQualityUnsubscribe = onLinkIndexChange(() => renderWikiQualityPanel())
    },
    onUnmount: () => {
      const body = $('wiki-quality-body')
      body?.removeEventListener('click', handleWikiQualityPanelEvent)
      body?.removeEventListener('keydown', handleWikiQualityPanelEvent)
      _wikiQualityUnsubscribe?.()
      _wikiQualityUnsubscribe = null
    },
    onRefresh: renderWikiQualityPanel,
  })

  // ── Bookmarks (gated) ────────────────────────────────────────
  setBookmarksOpenFile((item) => openFile(item))
  registerRightPanel('bookmarks', {
    title: 'Bookmarks',
    feature: 'featureBookmarks',
    icon: bookmarkIcon(),
    flex: 1,
    build: buildBookmarksPanel,
    onMount: mountBookmarksPanel,
    onUnmount: unmountBookmarksPanel,
    onRefresh: renderBookmarks,
  })

  // ── Calendar (gated) ─────────────────────────────────────────
  registerRightPanel('calendar', {
    title: 'Calendar',
    feature: 'featureCalendar',
    icon: calendarIcon(),
    flex: 0,
    build: () => `<div id="calendar-panel-body" class="widget-fill"></div>`,
    onMount: () => {
      const body = $('calendar-panel-body')
      if (body && !body.firstChild) {
        const panel = buildCalendarPanel({ onDateClick: (dateStr) => createDailyNote(dateStr), folderPath: state.folderPath })
        body.appendChild(panel)
      }
    },
    onUnmount: () => { $('calendar-panel-body')?.replaceChildren() },
  })
}
