import { useState } from "react";
import { Outlet, NavLink, useLocation } from "react-router-dom";
import {
  Box,
  Flex,
  Text,
  Container,
  IconButton,
} from "@radix-ui/themes";
import {
  HamburgerMenuIcon,
} from "@radix-ui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useDebug } from "../hooks/useDebug";
import { useAudioPlayer } from "../hooks/useAudioPlayer";
import { DebugPane } from "./DebugPane";
import { DrawerMenu } from "./DrawerMenu";
import { AudioPlayerBar } from "./AudioPlayerBar";

export function Layout() {
  const { debug, setDebug } = useDebug();
  const { currentTrack } = useAudioPlayer();
  const location = useLocation();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const hasAudioPlayer = currentTrack !== null;

  const isPlayer = location.pathname.startsWith("/player");

  return (
    <Flex direction="column" style={{ minHeight: "100vh" }}>
      {!isPlayer && (
        <div style={{ position: "fixed", inset: 0, zIndex: -1, background: "#09090b" }} />
      )}
      <DrawerMenu open={drawerOpen} onClose={() => setDrawerOpen(false)} />

      <Box
        asChild
        px="4"
        py="3"
        style={{
          borderBottom: "1px solid var(--gray-a5)",
          backdropFilter: "blur(12px)",
          position: "sticky",
          top: 0,
          zIndex: 100,
          background: "var(--color-panel-translucent)",
        }}
      >
        <header>
          <Container size="4">
            <Flex align="center" gap="3">
              <IconButton
                variant="ghost"
                size="2"
                onClick={() => setDrawerOpen(true)}
                aria-label="Open menu"
              >
                <HamburgerMenuIcon />
              </IconButton>

              <NavLink to="/" style={{ textDecoration: "none", color: "inherit" }}>
                <Flex align="center" gap="2">
                  <img src="/icons/logo.svg" alt="StreamX" width={28} height={28} />
                  <Text size="4" weight="bold">
                    StreamX
                  </Text>
                </Flex>
              </NavLink>
            </Flex>
          </Container>
        </header>
      </Box>

      <Box flexGrow="1" py="4" style={hasAudioPlayer ? { paddingBottom: 72 } : undefined}>
        <Container size="4" px="4">
          {isPlayer ? (
            <Outlet />
          ) : (
            <AnimatePresence mode="wait">
              <motion.div
                key={location.pathname}
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -8 }}
                transition={{ duration: 0.15 }}
              >
                <Outlet />
              </motion.div>
            </AnimatePresence>
          )}
        </Container>
      </Box>
      <AudioPlayerBar />
      {debug && <DebugPane onClose={() => setDebug(false)} />}
    </Flex>
  );
}
