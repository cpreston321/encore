# Features

Everything Encore does, in more detail than the [README](../README.md).

- **Your library.** Sign in with your Apple Music account to browse your Recently Added, Artists, Albums, Songs, playlists and Favorite Songs. You can also add to your library and playlists.
- **Full songs.** Subscribers hear complete tracks through Apple's native MusicKit player on macOS 14 and later, or through MusicKit JS in a hidden web view on macOS 13. Everyone else gets 30-second previews.
- **Crossfade and gapless playback.** Choose a 3, 6 or 12 second crossfade, or none. Consecutive album tracks hand off without a gap, which the native player does by itself. Since there's only one native player, a crossfade plays the incoming song on a web player while the outgoing one fades out, and the two swap each track.
- **Animate Artwork (optional).** View → Animate Artwork makes hero cards, album, playlist and artist headers, and Now Playing slowly drift and zoom, a stand-in for Apple Music's motion artwork, which the public API doesn't provide. It's off by default, pauses while Encore isn't the active app, and respects Reduce Motion.
- **Picks up where you left off.** Quit midway through a song and Encore reopens on it, paused at the same spot with the rest of the queue intact.
- **Synced lyrics.** Lyrics come from [LRCLIB](https://lrclib.net) and follow along with the song. Click a line to jump there.
- **Detail pages that take on the artwork's colors.** Album, artist and playlist pages tint to match their artwork, the header moves with parallax, and a compact title bar fades in as you scroll.
- **Quick search (⌘K).** Spotlight-style results show a top result, artists, albums, songs and playlists from both your library and the Apple Music catalog.
- **Easy to set up.** A new install opens on a welcome screen that signs you in and only shows the app once your library has loaded. Without a MusicKit key, it walks you through adding one: a link to Apple's key page, a file picker for the `.p8` file, and a check with Apple before saving.
- **A Mac app first.** Media keys and Control Center's Now Playing work. There are Dock menu controls, a menu bar item for every page, and full keyboard navigation.
- **Mini Player (⇧⌘M).** A small floating window with the cover, and the player bar's glass, scrubber and controls on hover. It stands in for the main window, which hides while it's open, and remembers where you put it.
- **Trackpad gestures.** Swipe sideways with two fingers to go back and forward, swipe the player to skip songs, scroll over the volume to change it, and shelves settle on a tile when you let go. Scroll views stretch and bounce at their edges, as in AppKit.
- **See All.** Every shelf opens a full page: 200 entries deep for the charts, and up to 100 of an artist's top songs.
- **Visualizer.** Hovering the progress bar shows a live spectrum of the song as it plays. Before volume control is on, or on the web player, it's computed from the song's preview instead.
- **Library tools.** Sort and filter your Songs, Favorites and Albums. ⌘-click or ⇧-click to select several songs, and drag songs onto a playlist or into Playing Next.
- **Fast and accessible.** Encore scrolls and animates at 120 fps on ProMotion displays. It launches in about a quarter of a second and uses less CPU than Music.app while playing ([benchmarks](benchmarks.md)). It follows the system's light or dark appearance and honors Reduce Motion.
