import Combine
import Foundation

final class FloatingBarViewModel: ObservableObject {
  @Published var amplitude: Double = 0
  @Published var status: FloatingBarStatus = .recording
  @Published var colorScheme: FloatingBarColorScheme = .dark
  @Published var isExpanded: Bool = false
  @Published var liveCaptionToggleVisible: Bool = false
  @Published var title: String = "Live transcript"
  @Published var transcriptBubbles: [FloatingTranscriptBubblePayload] = []
  @Published var isPillHovered: Bool = false
  // Tracks the panel's actual width so the SwiftUI container matches it while
  // the shrink-back resize is deliberately delayed past the collapse animation.
  @Published var pillHoverDisplayed: Bool = false
  @Published var startedAt: Date?
  let traceBuffer = InkTraceBuffer()
}
