import SwiftUI

enum SidebarDragItem: Equatable {
    case workspace(String)
    case thread(workspaceID: String, threadID: String)
}

struct WorkspaceSidebarDropDelegate: DropDelegate {
    let destinationWorkspaceID: String
    let orderedWorkspaceIDs: [String]
    @Binding var draggedItem: SidebarDragItem?
    let moveWorkspaces: (IndexSet, Int) -> Void

    func performDrop(info: DropInfo) -> Bool {
        guard
            case .workspace(let sourceWorkspaceID)? = draggedItem,
            sourceWorkspaceID != destinationWorkspaceID,
            let sourceIndex = orderedWorkspaceIDs.firstIndex(of: sourceWorkspaceID),
            let destinationIndex = orderedWorkspaceIDs.firstIndex(of: destinationWorkspaceID)
        else {
            draggedItem = nil
            return false
        }

        moveWorkspaces(
            IndexSet(integer: sourceIndex),
            moveDestinationIndex(forMoving: sourceIndex, onto: destinationIndex)
        )
        draggedItem = nil
        return true
    }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        DropProposal(operation: .move)
    }
}

struct ThreadSidebarDropDelegate: DropDelegate {
    let workspaceID: String
    let destinationThreadID: String
    let orderedThreadIDs: [String]
    @Binding var draggedItem: SidebarDragItem?
    let moveThreads: (String, IndexSet, Int) -> Void

    func performDrop(info: DropInfo) -> Bool {
        guard
            case .thread(let sourceWorkspaceID, let sourceThreadID)? = draggedItem,
            sourceWorkspaceID == workspaceID,
            sourceThreadID != destinationThreadID,
            let sourceIndex = orderedThreadIDs.firstIndex(of: sourceThreadID),
            let destinationIndex = orderedThreadIDs.firstIndex(of: destinationThreadID)
        else {
            draggedItem = nil
            return false
        }

        moveThreads(
            workspaceID,
            IndexSet(integer: sourceIndex),
            moveDestinationIndex(forMoving: sourceIndex, onto: destinationIndex)
        )
        draggedItem = nil
        return true
    }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        DropProposal(operation: .move)
    }
}

private func moveDestinationIndex(
    forMoving sourceIndex: Int,
    onto destinationIndex: Int
) -> Int {
    sourceIndex < destinationIndex ? destinationIndex + 1 : destinationIndex
}
