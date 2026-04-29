#if os(macOS)
  import Observation
  import SwiftUI

  struct FawxMacCommands: Commands {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel
    @Bindable var sparkleUpdater: SparkleUpdater
    @AppStorage("show_git_panel") private var showInspectorPanel = false

    var body: some Commands {
      CommandGroup(after: .appInfo) {
        Button("Check for Updates...") {
          sparkleUpdater.checkForUpdates()
        }
        .disabled(!sparkleUpdater.canCheckForUpdates)
      }

      CommandGroup(replacing: .newItem) {
        Button("New Thread") {
          beginNewThread()
        }
        .keyboardShortcut("n", modifiers: .command)
      }

      CommandMenu("Thread") {
        Button("Clear Thread History") {
          clearSelectedThread()
        }
        .keyboardShortcut("k", modifiers: .command)
        .disabled(sessionViewModel.selectedSessionID == nil)
      }

      CommandMenu("Navigate") {
        Button("Threads") {
          showThreads()
        }
        .keyboardShortcut("1", modifiers: .command)

        Button("Skills") {
          showSkills()
        }
        .keyboardShortcut("2", modifiers: .command)

        Button("Settings") {
          showSettings()
        }
        .keyboardShortcut("3", modifiers: .command)

        Divider()

        Button(showInspectorPanel ? "Hide Inspector" : "Show Inspector") {
          toggleInspector()
        }
        .keyboardShortcut("g", modifiers: [.command, .shift])
      }
    }

    private func beginNewThread() {
      sessionViewModel.select(nil)
      chatViewModel.showEmptyState()
    }

    private func showThreads() {
      if let chatSelection = sessionViewModel.currentChatSelection {
        appState.sidebarSelection = chatSelection
      } else {
        beginNewThread()
      }
    }

    private func showSkills() {
      appState.sidebarSelection = .skills
    }

    private func showSettings() {
      appState.sidebarSelection = .settings
    }

    private func toggleInspector() {
      GitPanelPresentation.toggle(
        showGitPanel: $showInspectorPanel,
        selectedSessionID: sessionViewModel.selectedSessionID,
        appState: appState,
        sessionViewModel: sessionViewModel,
        chatViewModel: chatViewModel
      )
    }

    private func clearSelectedThread() {
      guard let selectedSessionID = sessionViewModel.selectedSessionID else {
        return
      }

      Task { @MainActor in
        if chatViewModel.activeStreamSessionIDs.contains(selectedSessionID) {
          chatViewModel.stopStreaming(sessionID: selectedSessionID)
        }

        let didClear = await sessionViewModel.clearSession(id: selectedSessionID)
        if didClear {
          await chatViewModel.loadMessages(for: selectedSessionID, force: true)
        }
      }
    }

  }
#endif
