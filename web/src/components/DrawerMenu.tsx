import { useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import {
  Flex,
  Text,
  Badge,
  Separator,
} from "@radix-ui/themes";
import {
  CounterClockwiseClockIcon,
  DownloadIcon,
  GearIcon,
  ExitIcon,
  CodeIcon,
  StarFilledIcon,
} from "@radix-ui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useAuth } from "../hooks/useAuth";
import { useDebug } from "../hooks/useDebug";

interface DrawerMenuProps {
  open: boolean;
  onClose: () => void;
}

function VersionFooter() {
  const [version, setVersion] = useState<{ version: string; hash: string } | null>(
    null
  );
  useEffect(() => {
    fetch("/api/version")
      .then((r) => r.json())
      .then(setVersion)
      .catch(() => {});
  }, []);
  if (!version) return null;
  return (
    <Text
      size="1"
      color="gray"
      align="center"
      style={{ padding: "4px 0 8px", opacity: 0.7 }}
    >
      v{version.version} · {version.hash}
    </Text>
  );
}

const menuLinkStyle: React.CSSProperties = {
  textDecoration: "none",
  color: "inherit",
  display: "flex",
  alignItems: "center",
  gap: 12,
  padding: "10px 20px",
  borderRadius: 6,
  transition: "background 0.1s",
};

function MenuLink({
  to,
  icon,
  label,
  disabled,
  onClose,
}: {
  to: string;
  icon: React.ReactNode;
  label: string;
  disabled?: boolean;
  onClose: () => void;
}) {
  if (disabled) {
    return (
      <div style={{ ...menuLinkStyle, opacity: 0.35, cursor: "not-allowed" }}>
        {icon}
        <Text size="2">{label}</Text>
        <Badge size="1" variant="soft" color="gray" ml="auto">
          Soon
        </Badge>
      </div>
    );
  }

  return (
    <NavLink
      to={to}
      onClick={onClose}
      style={({ isActive }) => ({
        ...menuLinkStyle,
        background: isActive ? "var(--gray-a3)" : "transparent",
        fontWeight: isActive ? 600 : 400,
      })}
    >
      {icon}
      <Text size="2">{label}</Text>
    </NavLink>
  );
}

function FilmIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 15 15" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path
        d="M2 3a1 1 0 011-1h9a1 1 0 011 1v9a1 1 0 01-1 1H3a1 1 0 01-1-1V3zm1.5 0v2h2V3h-2zm3 0v2h2V3h-2zm3 0v2h2V3h-2zm2 3h-2v3h2V6zm0 4h-2v2h2v-2zm-3 2v-2h-2v2h2zm-3 0v-2h-2v2h2zm-2-3h2V6h-2v3zm3 0h2V6h-2v3z"
        fill="currentColor"
        fillRule="evenodd"
        clipRule="evenodd"
      />
    </svg>
  );
}

function MonitorIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 15 15" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path
        d="M1 3.5A1.5 1.5 0 012.5 2h10A1.5 1.5 0 0114 3.5v6a1.5 1.5 0 01-1.5 1.5h-10A1.5 1.5 0 011 9.5v-6zM2.5 3a.5.5 0 00-.5.5v6a.5.5 0 00.5.5h10a.5.5 0 00.5-.5v-6a.5.5 0 00-.5-.5h-10zM5 12.5a.5.5 0 01.5-.5h4a.5.5 0 010 1h-4a.5.5 0 01-.5-.5z"
        fill="currentColor"
        fillRule="evenodd"
        clipRule="evenodd"
      />
    </svg>
  );
}

function NoteIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 15 15" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path
        d="M8.5 1.5a.5.5 0 00-1 0V7a2.5 2.5 0 102 2.5V4h2a.5.5 0 000-1h-2.5a.5.5 0 00-.5.5v4A1.5 1.5 0 118.5 6V1.5z"
        fill="currentColor"
        fillRule="evenodd"
        clipRule="evenodd"
      />
    </svg>
  );
}

function SurroundIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 15 15" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path
        d="M7.5 1C4.5 1 2 3.5 2 7.5S4.5 14 7.5 14c1.2 0 2.3-.4 3.2-1M7.5 3.5C6 3.5 4.5 5 4.5 7.5S6 11.5 7.5 11.5c.8 0 1.5-.3 2-.8M7.5 6c-.5 0-1 .5-1 1.5s.5 1.5 1 1.5M10.7 2c1.5 1.2 2.3 3.2 2.3 5.5s-.8 4.3-2.3 5.5M12 4.5c.6.8 1 2 1 3s-.4 2.2-1 3"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinecap="round"
        fill="none"
      />
    </svg>
  );
}

export function DrawerMenu({ open, onClose }: DrawerMenuProps) {
  const { user, logout } = useAuth();
  const { debug, setDebug } = useDebug();

  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            onClick={onClose}
            style={{
              position: "fixed",
              inset: 0,
              background: "rgba(0, 0, 0, 0.25)",
              zIndex: 200,
            }}
          />
          <motion.div
            initial={{ x: "-100%" }}
            animate={{ x: 0 }}
            exit={{ x: "-100%" }}
            transition={{ type: "spring", damping: 30, stiffness: 300 }}
            style={{
              position: "fixed",
              top: 0,
              left: 0,
              bottom: 0,
              width: 280,
              background: "rgba(10, 10, 10, 0.82)",
              backdropFilter: "blur(20px) saturate(1.8)",
              WebkitBackdropFilter: "blur(20px) saturate(1.8)",
              zIndex: 201,
              display: "flex",
              flexDirection: "column",
              overflow: "auto",
              borderRight: "1px solid rgba(255,255,255,0.08)",
            }}
          >
            {/* User profile - same height as top menu */}
            <Flex align="center" gap="3" px="4" style={{ height: 52, borderBottom: "1px solid rgba(255,255,255,0.08)" }}>
              <div
                style={{
                  width: 30,
                  height: 30,
                  borderRadius: "50%",
                  border: "1.5px solid rgba(255,255,255,0.7)",
                  background: "transparent",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                }}
              >
                <svg width={15} height={15} viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <circle cx="12" cy="8" r="4"/>
                  <path d="M20 21a8 8 0 1 0-16 0"/>
                </svg>
              </div>
              <Flex align="center" gap="2">
                <Text size="2" weight="bold">
                  {user?.username ?? "User"}
                </Text>
                {user?.is_admin ? (
                  <svg width={9} height={9} viewBox="0 0 24 24" fill="none" stroke="var(--blue-9)" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
                  </svg>
                ) : (
                  <svg width={12} height={12} viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.4)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="12" cy="8" r="4"/>
                    <path d="M20 21a8 8 0 1 0-16 0"/>
                  </svg>
                )}
              </Flex>
            </Flex>

            <Separator size="4" />

            {/* Navigation */}
            <Flex direction="column" gap="0" py="2" px="2" style={{ flex: 1 }}>
              <MenuLink to="/" icon={<FilmIcon />} label="Movies" onClose={onClose} />
              <MenuLink to="/tv" icon={<MonitorIcon />} label="TV Shows" onClose={onClose} />
              <MenuLink to="/music" icon={<NoteIcon />} label="Music" onClose={onClose} />

              <Separator size="4" my="2" />

              <MenuLink to="/favourites" icon={<StarFilledIcon />} label="Favourites" onClose={onClose} />
              <MenuLink to="/downloads" icon={<DownloadIcon />} label="Downloads" onClose={onClose} />
              <MenuLink to="/history" icon={<CounterClockwiseClockIcon />} label="History" onClose={onClose} />

              <Separator size="4" my="2" />

              <MenuLink to="/surround" icon={<SurroundIcon />} label="Surround Sound" onClose={onClose} />
              <MenuLink to="/settings" icon={<GearIcon />} label="Settings" onClose={onClose} />
              {user?.is_admin && (
                <MenuLink to="/admin" icon={<svg width={15} height={15} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>} label="Admin" onClose={onClose} />
              )}

              <div
                onClick={() => setDebug(!debug)}
                style={{
                  ...menuLinkStyle,
                  cursor: "pointer",
                }}
              >
                <CodeIcon />
                <Text size="2">Debug Mode</Text>
                {debug && (
                  <Badge size="1" color="green" ml="auto">
                    ON
                  </Badge>
                )}
              </div>

              <div style={{ flex: 1 }} />

              <Separator size="4" my="2" />

              <div
                onClick={() => {
                  onClose();
                  logout();
                }}
                style={{
                  ...menuLinkStyle,
                  cursor: "pointer",
                  color: "var(--red-11)",
                }}
              >
                <ExitIcon />
                <Text size="2" color="red">
                  Logout
                </Text>
              </div>

              <VersionFooter />
            </Flex>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
