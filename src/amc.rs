use xcb::{randr, Xid};

macro_rules! xcb_make_request {
    ($conn:expr, $req:expr) => {
        $conn.wait_for_reply($conn.send_request($req))?
    };
}

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct XCBStack<'a> {
    pub conn: &'a xcb::Connection,
    pub root_screen: &'a xcb::x::Screen,
    pub root_window: &'a xcb::x::Window,
}

pub struct RandrOutputInfo {
    pub xres: randr::Output,
    pub info: randr::GetOutputInfoReply,
}

impl RandrOutputInfo {
    pub fn get_all(
        conn: &xcb::Connection,
        outputs: &[randr::Output],
    ) -> Result<Vec<RandrOutputInfo>> {
        let mut ret = Vec::<RandrOutputInfo>::new();
        for output in outputs {
            ret.push(RandrOutputInfo {
                xres: *output,
                info: xcb_make_request!(
                    conn,
                    &randr::GetOutputInfo {
                        output: *output,
                        config_timestamp: xcb::x::CURRENT_TIME,
                    }
                ),
            });
        }

        Ok(ret)
    }

    pub fn get_all_and_remove_dangling_crtcs(
        conn: &xcb::Connection,
        outputs: &[randr::Output],
    ) -> Result<Vec<RandrOutputInfo>> {
        let mut crtcs_removed = false;
        let mut outs = RandrOutputInfo::get_all(conn, outputs)?;

        for output in outs.iter() {
            if output.info.connection() != randr::Connection::Connected
                && !output.info.crtc().is_none()
            {
                crtcs_removed = true;
                conn.send_request(&randr::SetCrtcConfig {
                    crtc: output.info.crtc(),
                    timestamp: xcb::x::CURRENT_TIME,
                    config_timestamp: xcb::x::CURRENT_TIME,
                    x: 0,
                    y: 0,
                    mode: xcb::randr::Mode::none(),
                    rotation: xcb::randr::Rotation::ROTATE_0,
                    outputs: &[],
                });
            }
        }

        if crtcs_removed {
            /* Retrieve output info from X again in order to get rid
            of the dangling crtcs present on each affected output */
            outs = RandrOutputInfo::get_all(conn, outputs)?;
        }

        Ok(outs)
    }

    pub fn get_best_mode(
        &self,
        xstack: &XCBStack,
        modes: &[randr::ModeInfo],
    ) -> Result<(randr::Mode, u16, u16)> {
        // code "adapted" from the xrandr source
        let mut best_mode: Option<&randr::Mode> = None;
        let mut best_mode_info: Option<&randr::ModeInfo> = None;
        let mut best_dist = 0;

        for i in 0..self.info.modes().len() {
            let mode_info = match modes
                .iter()
                .find(|mode_info| mode_info.id == self.info.modes()[i].resource_id())
            {
                Some(x) => x,
                None => continue,
            };

            let mut dist: i32;

            if i < self.info.num_preferred().into() {
                dist = 0;
            } else if self.info.mm_height() > 0 {
                dist = (1000 * (xstack.root_screen.height_in_pixels() as i32)
                    / (xstack.root_screen.height_in_millimeters() as i32))
                    - (1000 * (mode_info.height as i32) / (self.info.mm_height() as i32));
            } else {
                dist = (xstack.root_screen.height_in_pixels() as i32) - (mode_info.height as i32);
            }

            if dist < 0 {
                dist *= -1;
            }

            if best_mode.is_none() || dist < best_dist {
                best_mode = Some(&self.info.modes()[i]);
                best_mode_info = Some(mode_info);
                best_dist = dist;
            }
        }

        if best_mode.is_none() {
            return Err("Couldn't find a best mode.".into());
        }

        if best_mode_info.is_none() {
            return Err("Couldn't find best mode info but found a best mode?.".into());
        }

        return Ok((
            *best_mode.unwrap(),
            best_mode_info.unwrap().width,
            best_mode_info.unwrap().height,
        ));
    }
}

#[derive(Debug)]
pub struct MonitorCrtcConfig {
    pub x: i16,
    pub y: i16,
    pub rot: randr::Rotation,
}

#[derive(Debug)]
pub struct Monitor {
    pub id: u32,
    pub name: String,
    /* The monitor's current configuration. If it is unconfigured this is None */
    pub crtc_config: Option<MonitorCrtcConfig>,
    pub output: randr::Output,
    pub mode_best: randr::Mode,
    pub crtc_slot: randr::Crtc,
    pub w: u16,
    pub h: u16,
    pub w_mm: u32,
    pub h_mm: u32,
}

impl Monitor {
    fn build(
        xstack: &XCBStack,
        id: u32,
        output: &RandrOutputInfo,
        crtc_slot: Option<&randr::Crtc>,
        modes: &[randr::ModeInfo],
    ) -> Result<(Monitor, bool)> {
        let (best_mode, width, height) = output.get_best_mode(xstack, modes)?;

        if output.info.crtc().is_none() {
            if crtc_slot.is_none() {
                return Err("Output needed a new crtc as it was not configured, but there were none left. Can your GPU handle this many monitors?".into());
            }

            Ok((
                Monitor {
                    id,
                    name: String::from_utf8(output.info.name().to_vec()).unwrap(),
                    crtc_config: None,
                    output: output.xres,
                    crtc_slot: *crtc_slot.unwrap(),
                    mode_best: best_mode,
                    w: width,
                    h: height,
                    w_mm: output.info.mm_width(),
                    h_mm: output.info.mm_height(),
                },
                true,
            ))
        } else {
            let crtc_info = xcb_make_request!(
                xstack.conn,
                &randr::GetCrtcInfo {
                    crtc: output.info.crtc(),
                    config_timestamp: xcb::x::CURRENT_TIME,
                }
            );

            Ok((
                Monitor {
                    id,
                    name: String::from_utf8(output.info.name().to_vec()).unwrap(),
                    output: output.xres,
                    crtc_config: Some(MonitorCrtcConfig {
                        x: crtc_info.x(),
                        y: crtc_info.y(),
                        rot: crtc_info.rotation(),
                    }),
                    crtc_slot: output.info.crtc(),
                    mode_best: best_mode,
                    w: width,
                    h: height,
                    w_mm: output.info.mm_width(),
                    h_mm: output.info.mm_height(),
                },
                false,
            ))
        }
    }

    fn make_id_from_edid(edid_bytes: &[u8]) -> u32 {
        let mut digest = 0;

        for byte in edid_bytes {
            let mut tmp = u32::wrapping_add(*byte as u32, digest << 6);
            tmp = u32::wrapping_add(tmp, digest << 16);
            tmp = u32::wrapping_sub(tmp, digest);
            digest = tmp;
        }

        return digest;
    }

    pub fn get_all_connected(xstack: &XCBStack) -> Result<Vec<Monitor>> {
        let screen_resources = xcb_make_request!(
            xstack.conn,
            &randr::GetScreenResources {
                window: *xstack.root_window,
            }
        );

        let outputs = RandrOutputInfo::get_all_and_remove_dangling_crtcs(
            xstack.conn,
            screen_resources.outputs(),
        )?;

        let mut free_crtcs: Vec<randr::Crtc> = Vec::from(screen_resources.crtcs())
            .into_iter()
            .filter(|&crtc| {
                outputs
                    .iter()
                    .find(|output| output.info.crtc() == crtc)
                    .is_none()
            })
            .collect();

        let mut connected_mons = Vec::<Monitor>::with_capacity(screen_resources.outputs().len());

        for output in outputs {
            if output.info.connection() != randr::Connection::Connected {
                continue;
            }

            let output_props = xcb_make_request!(
                xstack.conn,
                &randr::ListOutputProperties {
                    output: output.xres
                }
            );

            let mut mon_id: u32 = 0;
            for atom in output_props.atoms() {
                let atom_name =
                    xcb_make_request!(xstack.conn, &xcb::x::GetAtomName { atom: *atom });

                if atom_name.name().as_ascii() != "EDID" {
                    continue;
                }

                let edid_data = xcb_make_request!(
                    xstack.conn,
                    &randr::GetOutputProperty {
                        output: output.xres,
                        property: *atom,
                        r#type: xcb::x::ATOM_ANY,
                        long_offset: 0,
                        long_length: 100,
                        delete: false,
                        pending: false,
                    }
                );

                assert!(edid_data.r#type() == xcb::x::ATOM_INTEGER && edid_data.format() == 8);

                mon_id = Self::make_id_from_edid(edid_data.data::<u8>());
                break;
            }

            assert!(mon_id != 0);

            match Self::build(
                xstack,
                mon_id,
                &output,
                match free_crtcs.len() {
                    0 => None,
                    _ => Some(&free_crtcs[0]),
                },
                screen_resources.modes(),
            ) {
                Ok((mon, consumed_crtc)) => {
                    if consumed_crtc {
                        free_crtcs.remove(0);
                    }

                    connected_mons.push(mon);
                }
                Err(err) => return Err(err),
            }
        }

        return Ok(connected_mons);
    }

    pub fn apply_config(&self, xstack: &XCBStack, conf: &MonitorConfig) -> Result<bool> {
        let configure = match &self.crtc_config {
            Some(cur_config) => {
                conf.x != cur_config.x || conf.y != cur_config.y || conf.rot != cur_config.rot
            }
            None => true,
        };

        if !configure {
            return Ok(false);
        }

        xcb_make_request!(
            xstack.conn,
            &randr::SetCrtcConfig {
                crtc: self.crtc_slot,
                timestamp: xcb::x::CURRENT_TIME,
                config_timestamp: xcb::x::CURRENT_TIME,
                x: conf.x,
                y: conf.y,
                mode: self.mode_best,
                rotation: conf.rot,
                outputs: &[self.output],
            }
        );

        return Ok(true);
    }
}

#[derive(Debug)]
pub struct MonitorConfig {
    pub id: u32,
    pub x: i16,
    pub y: i16,
    pub rot: randr::Rotation,
}
