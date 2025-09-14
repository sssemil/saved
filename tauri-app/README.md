# SAVED

**SAVED - A p2p e2ee notes and files sync app**

A secure, peer-to-peer, end-to-end encrypted notes and files synchronization application built with Tauri, React, and TypeScript.

## Features

- 🔒 **End-to-End Encryption**: Your data is encrypted before leaving your device
- 🌐 **Peer-to-Peer Sync**: Direct device-to-device synchronization without central servers
- 📝 **Notes Management**: Create, edit, and organize your notes securely
- 📁 **File Sync**: Synchronize files across your devices
- 🖥️ **Cross-Platform**: Available on desktop and mobile platforms
- ⚡ **Fast & Lightweight**: Built with modern technologies for optimal performance

## Technology Stack

- **Frontend**: React + TypeScript + Vite
- **Backend**: Rust + Tauri
- **Encryption**: End-to-end encryption for data security
- **Sync**: Peer-to-peer synchronization protocol

## Development

### Prerequisites

- Node.js (v18 or higher)
- Rust (latest stable)
- Android SDK (for mobile builds)

### Getting Started

1. Clone the repository
2. Install dependencies:
   ```bash
   npm install
   ```

3. Start development server:
   ```bash
   npm run dev
   ```

4. Build for production:
   ```bash
   npm run build
   ```

### Building for Android

```bash
cargo tauri android build --apk
```

## Security

SAVED prioritizes your privacy and security:
- All data is encrypted locally before synchronization
- No central servers store your data
- Peer-to-peer connections ensure direct device communication
- Open source for transparency and community review

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Support

For support and questions, please open an issue on GitHub.