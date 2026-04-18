import { useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { KeepAliveOutlet } from "./KeepAliveOutlet";
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
import { useAuth } from "../hooks/useAuth";
import { useDebug } from "../hooks/useDebug";
import { useAudioPlayer } from "../hooks/useAudioPlayer";
import { useVersionCheck } from "../hooks/useVersionCheck";
import { LOGO_URL, PAGE_BG_URL } from "../assets";
import { DebugPane } from "./DebugPane";
import { DrawerMenu } from "./DrawerMenu";
import { AudioPlayerBar } from "./AudioPlayerBar";

export function Layout() {
  const { debug, setDebug } = useDebug();
  const { isGuest } = useAuth();
  const { currentTrack } = useAudioPlayer();
  const location = useLocation();
  const navigate = useNavigate();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [guestBannerDismissed, setGuestBannerDismissed] = useState(false);
  const hasAudioPlayer = currentTrack !== null;

  const isPlayer = location.pathname.startsWith("/player");
  const { updateAvailable, reload } = useVersionCheck();

  return (
    <Flex direction="column" style={{ minHeight: "100vh" }}>
      {!isPlayer && (
        <div style={{ position: "fixed", inset: 0, zIndex: -1 }}>
          <img
            src={PAGE_BG_URL}
            alt=""
            style={{
              position: "absolute",
              inset: "-10%",
              width: "120%",
              height: "120%",
              objectFit: "cover",
              filter: "blur(30px) brightness(0.12) saturate(1.2)",
            }}
          />
          <div style={{ position: "absolute", inset: 0, background: "rgba(0,0,0,0.4)" }} />
        </div>
      )}
      <DrawerMenu open={drawerOpen} onClose={() => setDrawerOpen(false)} />

      <div
        style={{
          position: "sticky",
          top: 0,
          zIndex: 100,
        }}
      >
        <Box
          asChild
          px="4"
          py="2"
          style={{
            borderBottom: updateAvailable && !isPlayer ? undefined : "1px solid var(--gray-a5)",
            backdropFilter: "blur(12px)",
            background: "var(--color-panel-translucent)",
          }}
        >
          <header>
            <Container size="4">
              <Flex align="center" justify="between">
                <Flex
                  align="center"
                  gap="1"
                  onClick={() => setDrawerOpen(true)}
                  style={{ lineHeight: 1, cursor: "pointer", textDecoration: "none", color: "inherit" }}
                >
                  <img src={LOGO_URL} alt="StreamX" width={36} height={36} style={{ display: "block" }} />
                  <Text size="4" weight="bold" style={{ lineHeight: 1, position: "relative", top: 3, left: -3 }}>
                    StreamX
                  </Text>
                </Flex>
                <IconButton
                  variant="ghost"
                  size="2"
                  onClick={() => setDrawerOpen(true)}
                  aria-label="Open menu"
                  style={{ color: "white", position: "relative", left: -7 }}
                >
                  <HamburgerMenuIcon />
                </IconButton>
              </Flex>
            </Container>
          </header>
        </Box>

        {updateAvailable && !isPlayer && (
          <Flex
            align="center"
            justify="center"
            gap="3"
            py="2"
            onClick={reload}
            style={{
              background: "var(--accent-9)",
              cursor: "pointer",
              borderBottom: "1px solid var(--gray-a5)",
            }}
          >
            <Text size="2" weight="medium" style={{ color: "white" }}>
              New version available
            </Text>
            <Text size="1" style={{ color: "rgba(255,255,255,0.8)", textDecoration: "underline" }}>
              Refresh
            </Text>
          </Flex>
        )}

        {isGuest && !guestBannerDismissed && (
          <Flex
            align="center"
            justify="center"
            gap="3"
            py="2"
            px="3"
            style={{
              background: "var(--orange-9)",
              borderBottom: "1px solid var(--gray-a5)",
            }}
          >
            <Text
              size="2"
              weight="medium"
              style={{ color: "white", cursor: "pointer", textDecoration: "underline" }}
              onClick={() => navigate("/login?tab=register")}
            >
              Create a Free Account to access all features
            </Text>
            <div
              onClick={() => setGuestBannerDismissed(true)}
              style={{ cursor: "pointer", padding: 2, color: "white", opacity: 0.8 }}
            >
              &#10005;
            </div>
          </Flex>
        )}
      </div>
      <Box flexGrow="1" py="4" style={hasAudioPlayer ? { paddingBottom: 72 } : undefined}>
        <Container size="4" px="4">
          {isPlayer ? (
            <Outlet />
          ) : (
            <KeepAliveOutlet />
          )}
        </Container>
      </Box>
      <AudioPlayerBar />
      {debug && <DebugPane onClose={() => setDebug(false)} />}
    </Flex>
  );
}
