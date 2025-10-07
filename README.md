# SAVED

Goal 1:
    Create an engine using libp2p which would allow you to easily connect two peers over LAN or the Internet, while
    also allowing to set whitelisted peers. It should use the libp2p norms.

Issue:
    With so many addresses available from different systems, how do we prioritize them?

Issue:
    We need to have two modes for the network, active and passive. Active mode must be used when a node from target
    nodes is not connected to us, in this mode, actively look on the DHT, and reserve relays. Once we are connected to
    all target nodes, go into passive mode. In passive mode, just maintain a connection to target nodes.
